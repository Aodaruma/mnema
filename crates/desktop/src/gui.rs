use std::path::PathBuf;

use anyhow::{Result, anyhow};
use eframe::egui::{self, Align, Color32, RichText, ScrollArea, TextEdit};
use mnema_app::{
    CaptureTaskRequest, CaptureTaskService, PlanTodayRequest, PlanTodayResult,
    ProposedScheduleBlock, SchedulePlanStoreService,
};
use mnema_core::prelude::*;
use mnema_infra::db::{StorageBackend, Vault};
use time::{Date, OffsetDateTime, macros::format_description};
use tokio::runtime::Runtime;

pub fn run_gui(initial_vault_path: PathBuf) -> Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1120.0, 760.0])
            .with_min_inner_size([860.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Mnema",
        native_options,
        Box::new(move |cc| Ok(Box::new(MnemaGuiApp::new(cc, initial_vault_path)))),
    )
    .map_err(|error| anyhow!(error.to_string()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Today,
    Inbox,
    Schedule,
    Settings,
}

struct MnemaGuiApp {
    runtime: Runtime,
    vault_path: String,
    vault: Option<Vault>,
    backend: Option<StorageBackend>,
    view: View,
    task_title: String,
    due_date: String,
    minutes: String,
    target_date: String,
    tasks: Vec<Task>,
    plan: Option<PlanTodayResult>,
    schedule: Vec<ScheduleBlock>,
    message: String,
    error: Option<String>,
}

impl MnemaGuiApp {
    fn new(cc: &eframe::CreationContext<'_>, initial_vault_path: PathBuf) -> Self {
        configure_style(&cc.egui_ctx);

        let today = OffsetDateTime::now_utc().date().to_string();
        let runtime = Runtime::new().expect("tokio runtime must initialize for Mnema GUI");
        let mut app = Self {
            runtime,
            vault_path: initial_vault_path.display().to_string(),
            vault: None,
            backend: None,
            view: View::Today,
            task_title: String::new(),
            due_date: today.clone(),
            minutes: String::from("45"),
            target_date: today,
            tasks: Vec::new(),
            plan: None,
            schedule: Vec::new(),
            message: String::new(),
            error: None,
        };
        app.connect_and_refresh();
        app
    }

    fn connect_and_refresh(&mut self) {
        let path = self.normalized_vault_path();
        let result = self.runtime.block_on(async move {
            let vault = Vault::connect_or_init(&path).await?;
            vault.initialize_defaults().await?;
            Result::<Vault>::Ok(vault)
        });

        match result {
            Ok(vault) => {
                self.backend = Some(vault.backend());
                self.vault = Some(vault);
                self.message = String::from("Vault connected");
                self.error = None;
                self.refresh_tasks();
                self.refresh_schedule();
            }
            Err(error) => self.set_error(error),
        }
    }

    fn refresh_tasks(&mut self) {
        let Ok(vault) = self.vault_clone() else {
            return;
        };
        let result = self.runtime.block_on(async move {
            let task_repo = vault.task_repo();
            let mut tasks = task_repo.list_all().await?;
            tasks.retain(|task| task.deleted_at.is_none());
            tasks.sort_by_key(|task| (task.due_date, task.created_at));
            Result::<Vec<Task>>::Ok(tasks)
        });

        match result {
            Ok(tasks) => {
                self.tasks = tasks;
                self.error = None;
            }
            Err(error) => self.set_error(error),
        }
    }

    fn refresh_schedule(&mut self) {
        let Ok(vault) = self.vault_clone() else {
            return;
        };
        let target_date = match parse_required_date(&self.target_date) {
            Ok(date) => date,
            Err(error) => {
                self.set_error(error);
                return;
            }
        };

        let result = self.runtime.block_on(async move {
            let schedule_block_repo = vault.schedule_block_repo();
            let store = SchedulePlanStoreService::new(schedule_block_repo.as_ref());
            Result::<Vec<ScheduleBlock>>::Ok(store.list_for_day(target_date).await?)
        });

        match result {
            Ok(schedule) => {
                self.schedule = schedule;
                self.error = None;
            }
            Err(error) => self.set_error(error),
        }
    }

    fn add_task(&mut self) {
        let title = self.task_title.trim().to_string();
        if title.is_empty() {
            self.set_error(anyhow!("task title is required"));
            return;
        }

        let due_date = match parse_optional_date(&self.due_date) {
            Ok(date) => date,
            Err(error) => {
                self.set_error(error);
                return;
            }
        };
        let estimated_minutes = match parse_optional_u32(&self.minutes) {
            Ok(minutes) => minutes,
            Err(error) => {
                self.set_error(error);
                return;
            }
        };
        let Ok(vault) = self.vault_clone() else {
            return;
        };

        let result = self.runtime.block_on(async move {
            vault.initialize_defaults().await?;
            let task_repo = vault.task_repo();
            let list_repo = vault.list_repo();
            let status_repo = vault.status_repo();
            let service = CaptureTaskService::new(
                task_repo.as_ref(),
                list_repo.as_ref(),
                status_repo.as_ref(),
            );
            Result::<Task>::Ok(
                service
                    .capture_inbox_task(CaptureTaskRequest {
                        title,
                        description: None,
                        due_date,
                        estimated_minutes,
                    })
                    .await?
                    .task,
            )
        });

        match result {
            Ok(task) => {
                self.task_title.clear();
                self.message = format!("Added: {}", task.title);
                self.error = None;
                self.refresh_tasks();
            }
            Err(error) => self.set_error(error),
        }
    }

    fn plan_today(&mut self, save: bool) {
        let Ok(vault) = self.vault_clone() else {
            return;
        };
        let target_date = match parse_required_date(&self.target_date) {
            Ok(date) => date,
            Err(error) => {
                self.set_error(error);
                return;
            }
        };
        let availability = match default_workday_availability(target_date) {
            Ok(availability) => availability,
            Err(error) => {
                self.set_error(error);
                return;
            }
        };

        let result = self.runtime.block_on(async move {
            let task_repo = vault.task_repo();
            let status_repo = vault.status_repo();
            let service =
                mnema_app::PlanTodayService::new(task_repo.as_ref(), status_repo.as_ref());
            let plan = service
                .plan_today(PlanTodayRequest {
                    target_date,
                    availability,
                    busy_blocks: Vec::new(),
                })
                .await?;

            let saved = if save {
                let schedule_block_repo = vault.schedule_block_repo();
                let store = SchedulePlanStoreService::new(schedule_block_repo.as_ref());
                store.save_proposed_plan(&plan).await?.len()
            } else {
                0
            };

            Result::<(PlanTodayResult, usize)>::Ok((plan, saved))
        });

        match result {
            Ok((plan, saved)) => {
                let block_count = plan.output.blocks.len();
                self.plan = Some(plan);
                self.message = if save {
                    format!("Saved {saved} proposed blocks")
                } else {
                    format!("Planned {block_count} blocks")
                };
                self.error = None;
                if save {
                    self.refresh_schedule();
                }
            }
            Err(error) => self.set_error(error),
        }
    }

    fn normalized_vault_path(&self) -> PathBuf {
        let trimmed = self.vault_path.trim();
        if trimmed.is_empty() {
            PathBuf::from("./vault")
        } else {
            PathBuf::from(trimmed)
        }
    }

    fn vault_clone(&mut self) -> Result<Vault> {
        self.vault
            .clone()
            .ok_or_else(|| anyhow!("vault is not connected"))
            .map_err(|error| {
                self.set_error(anyhow!(error.to_string()));
                error
            })
    }

    fn set_error(&mut self, error: anyhow::Error) {
        self.error = Some(error.to_string());
    }
}

impl eframe::App for MnemaGuiApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("top_bar").show_inside(ui, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.heading(RichText::new("Mnema").color(Color32::from_rgb(24, 35, 53)));
                ui.add_space(12.0);
                ui.label(format!("Vault: {}", self.normalized_vault_path().display()));
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Refresh").clicked() {
                        self.refresh_tasks();
                        self.refresh_schedule();
                    }
                    let backend = self
                        .backend
                        .map(|backend| match backend {
                            StorageBackend::Sqlite => "SQLite",
                            StorageBackend::Postgres => "PostgreSQL",
                        })
                        .unwrap_or("Disconnected");
                    ui.label(backend);
                });
            });
            ui.add_space(8.0);
        });

        egui::Panel::left("navigation")
            .resizable(false)
            .exact_size(168.0)
            .show_inside(ui, |ui| {
                ui.add_space(12.0);
                nav_button(ui, &mut self.view, View::Today, "Today");
                nav_button(ui, &mut self.view, View::Inbox, "Inbox");
                nav_button(ui, &mut self.view, View::Schedule, "Schedule");
                ui.separator();
                nav_button(ui, &mut self.view, View::Settings, "Settings");
            });

        egui::Panel::bottom("status_bar").show_inside(ui, |ui| {
            ui.add_space(6.0);
            if let Some(error) = &self.error {
                ui.colored_label(Color32::from_rgb(176, 48, 48), error);
            } else {
                ui.label(&self.message);
            }
            ui.add_space(6.0);
        });

        egui::CentralPanel::default().show_inside(ui, |ui| match self.view {
            View::Today => self.show_today(ui),
            View::Inbox => self.show_inbox(ui),
            View::Schedule => self.show_schedule(ui),
            View::Settings => self.show_settings(ui),
        });
    }
}

impl MnemaGuiApp {
    fn show_today(&mut self, ui: &mut egui::Ui) {
        section_header(ui, "Today");
        ui.horizontal(|ui| {
            ui.label("Date");
            ui.add_sized([120.0, 28.0], TextEdit::singleline(&mut self.target_date));
            if ui.button("Plan").clicked() {
                self.plan_today(false);
            }
            if ui.button("Save").clicked() {
                self.plan_today(true);
            }
        });
        ui.add_space(12.0);

        if let Some(plan) = &self.plan {
            if !plan.output.issues.is_empty() {
                ui.colored_label(
                    Color32::from_rgb(145, 92, 24),
                    format!("Issues: {}", plan.output.issues.len()),
                );
            }
            if !plan.output.unscheduled.is_empty() {
                ui.label(format!("Unscheduled: {}", plan.output.unscheduled.len()));
            }
            block_list(ui, &plan.output.blocks);
        } else {
            ui.label("No plan yet.");
        }

        ui.separator();
        ui.label(RichText::new("Tasks").strong());
        task_list(ui, &self.tasks);
    }

    fn show_inbox(&mut self, ui: &mut egui::Ui) {
        section_header(ui, "Inbox");
        ui.horizontal(|ui| {
            ui.add_sized(
                [340.0, 30.0],
                TextEdit::singleline(&mut self.task_title).hint_text("Task title"),
            );
            ui.add_sized(
                [124.0, 30.0],
                TextEdit::singleline(&mut self.due_date).hint_text("YYYY-MM-DD"),
            );
            ui.add_sized(
                [64.0, 30.0],
                TextEdit::singleline(&mut self.minutes).hint_text("min"),
            );
            if ui.button("Add").clicked() {
                self.add_task();
            }
        });
        ui.add_space(12.0);
        task_list(ui, &self.tasks);
    }

    fn show_schedule(&mut self, ui: &mut egui::Ui) {
        section_header(ui, "Schedule");
        ui.horizontal(|ui| {
            ui.label("Date");
            ui.add_sized([120.0, 28.0], TextEdit::singleline(&mut self.target_date));
            if ui.button("Load").clicked() {
                self.refresh_schedule();
            }
        });
        ui.add_space(12.0);

        if self.schedule.is_empty() {
            ui.label("No saved schedule blocks.");
            return;
        }

        ScrollArea::vertical().show(ui, |ui| {
            for block in &self.schedule {
                ui.horizontal(|ui| {
                    ui.monospace(format!(
                        "{}-{}",
                        format_hm(block.start_at),
                        format_hm(block.end_at)
                    ));
                    ui.label(
                        block
                            .title_snapshot
                            .as_deref()
                            .unwrap_or("(untitled block)"),
                    );
                    ui.label(
                        RichText::new(schedule_state_label(&block.state))
                            .color(Color32::from_rgb(34, 112, 83)),
                    );
                });
                ui.separator();
            }
        });
    }

    fn show_settings(&mut self, ui: &mut egui::Ui) {
        section_header(ui, "Settings");
        ui.horizontal(|ui| {
            ui.label("Vault");
            ui.add_sized([520.0, 30.0], TextEdit::singleline(&mut self.vault_path));
            if ui.button("Open").clicked() {
                self.connect_and_refresh();
            }
        });
        ui.add_space(10.0);
        ui.label(format!(
            "Backend: {}",
            self.backend
                .map(|backend| match backend {
                    StorageBackend::Sqlite => "SQLite",
                    StorageBackend::Postgres => "PostgreSQL",
                })
                .unwrap_or("Disconnected")
        ));
        ui.horizontal(|ui| {
            ui.label("SQLite path env");
            ui.monospace("MNEMA_SQLITE_PATH");
        });
        ui.horizontal(|ui| {
            ui.label("Backend env");
            ui.monospace("MNEMA_STORAGE_BACKEND");
        });
    }
}

fn configure_style(ctx: &egui::Context) {
    let mut style = (*ctx.global_style()).clone();
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 7.0);
    style.visuals = egui::Visuals::light();
    style.visuals.selection.bg_fill = Color32::from_rgb(44, 110, 157);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(44, 110, 157);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(226, 234, 241);
    ctx.set_global_style(style);
}

fn nav_button(ui: &mut egui::Ui, view: &mut View, target: View, label: &str) {
    let selected = *view == target;
    if ui
        .add_sized([140.0, 32.0], egui::Button::selectable(selected, label))
        .clicked()
    {
        *view = target;
    }
}

fn section_header(ui: &mut egui::Ui, title: &str) {
    ui.add_space(12.0);
    ui.heading(RichText::new(title).color(Color32::from_rgb(31, 42, 68)));
    ui.add_space(8.0);
}

fn task_list(ui: &mut egui::Ui, tasks: &[Task]) {
    if tasks.is_empty() {
        ui.label("No tasks.");
        return;
    }

    ScrollArea::vertical().show(ui, |ui| {
        for task in tasks {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&task.title).strong());
                if let Some(due_date) = task.due_date {
                    ui.label(
                        RichText::new(format!("due {due_date}"))
                            .color(Color32::from_rgb(122, 74, 32)),
                    );
                }
                if let Some(minutes) = task.estimated_minutes {
                    ui.label(format!("{minutes}m"));
                }
            });
            ui.separator();
        }
    });
}

fn block_list(ui: &mut egui::Ui, blocks: &[ProposedScheduleBlock]) {
    if blocks.is_empty() {
        ui.label("No scheduled blocks.");
        return;
    }

    ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
        for block in blocks {
            ui.horizontal(|ui| {
                ui.monospace(format!(
                    "{}-{}",
                    format_hm(block.window.start),
                    format_hm(block.window.end)
                ));
                ui.label(RichText::new(&block.title).strong());
                ui.label(format!("{}m", block.required_minutes));
            });
            ui.separator();
        }
    });
}

fn parse_optional_date(value: &str) -> Result<Option<Date>> {
    let value = value.trim();
    if value.is_empty() {
        Ok(None)
    } else {
        parse_required_date(value).map(Some)
    }
}

fn parse_required_date(value: &str) -> Result<Date> {
    Date::parse(value.trim(), format_description!("[year]-[month]-[day]")).map_err(Into::into)
}

fn parse_optional_u32(value: &str) -> Result<Option<u32>> {
    let value = value.trim();
    if value.is_empty() {
        Ok(None)
    } else {
        value.parse::<u32>().map(Some).map_err(Into::into)
    }
}

fn default_workday_availability(date: Date) -> Result<Vec<mnema_app::AvailabilityWindow>> {
    let start = date.with_hms(9, 0, 0)?.assume_utc();
    let end = date.with_hms(17, 0, 0)?.assume_utc();
    Ok(vec![mnema_app::AvailabilityWindow {
        window: mnema_app::TimeWindow::new(start, end),
    }])
}

fn format_hm(value: OffsetDateTime) -> String {
    value
        .format(format_description!("[hour]:[minute]"))
        .unwrap_or_else(|_| value.time().to_string())
}

fn schedule_state_label(state: &ScheduleBlockState) -> &'static str {
    match state {
        ScheduleBlockState::Proposed => "proposed",
        ScheduleBlockState::Scheduled => "scheduled",
        ScheduleBlockState::Active => "active",
        ScheduleBlockState::Done => "done",
        ScheduleBlockState::Missed => "missed",
        ScheduleBlockState::Cancelled => "cancelled",
    }
}
