use std::path::PathBuf;

use anyhow::{Result, anyhow};
use eframe::egui::{self, Align, Color32, RichText, ScrollArea, Stroke, TextEdit};
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
    dark_mode: bool,
}

impl MnemaGuiApp {
    fn new(cc: &eframe::CreationContext<'_>, initial_vault_path: PathBuf) -> Self {
        let dark_mode = cc.egui_ctx.theme() == egui::Theme::Dark;
        configure_style(&cc.egui_ctx, if dark_mode { 1.0 } else { 0.0 });

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
            dark_mode,
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
        let dark_factor = ui.ctx().animate_bool_with_time(
            egui::Id::new("mnema_theme_transition"),
            self.dark_mode,
            0.28,
        );
        configure_style(ui.ctx(), dark_factor);
        if (0.0..1.0).contains(&dark_factor) {
            ui.ctx().request_repaint();
        }
        let palette = Palette::at(dark_factor);

        egui::Panel::top("top_bar").show_inside(ui, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.heading(RichText::new("Mnema").color(palette.brand));
                ui.add_space(12.0);
                ui.label(format!("Vault: {}", self.normalized_vault_path().display()));
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    theme_toggle(ui, &mut self.dark_mode, palette);
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
                ui.colored_label(palette.error, error);
            } else {
                ui.label(&self.message);
            }
            ui.add_space(6.0);
        });

        egui::CentralPanel::default().show_inside(ui, |ui| match self.view {
            View::Today => self.show_today(ui, palette),
            View::Inbox => self.show_inbox(ui, palette),
            View::Schedule => self.show_schedule(ui, palette),
            View::Settings => self.show_settings(ui, palette),
        });
    }
}

impl MnemaGuiApp {
    fn show_today(&mut self, ui: &mut egui::Ui, palette: Palette) {
        section_header(ui, "Today", palette);
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
                    palette.warning,
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
        task_list(ui, &self.tasks, palette);
    }

    fn show_inbox(&mut self, ui: &mut egui::Ui, palette: Palette) {
        section_header(ui, "Inbox", palette);
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
        task_list(ui, &self.tasks, palette);
    }

    fn show_schedule(&mut self, ui: &mut egui::Ui, palette: Palette) {
        section_header(ui, "Schedule", palette);
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
                        RichText::new(schedule_state_label(&block.state)).color(palette.success),
                    );
                });
                ui.separator();
            }
        });
    }

    fn show_settings(&mut self, ui: &mut egui::Ui, palette: Palette) {
        section_header(ui, "Settings", palette);
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

#[derive(Debug, Clone, Copy)]
struct Palette {
    text: Color32,
    muted: Color32,
    brand: Color32,
    section: Color32,
    panel: Color32,
    surface: Color32,
    faint: Color32,
    input: Color32,
    code_bg: Color32,
    control_bg: Color32,
    control_hover: Color32,
    control_active: Color32,
    border: Color32,
    border_strong: Color32,
    accent: Color32,
    selected_fill: Color32,
    selected_text: Color32,
    warning: Color32,
    success: Color32,
    due: Color32,
    error: Color32,
}

impl Palette {
    fn at(dark_factor: f32) -> Self {
        let t = dark_factor.clamp(0.0, 1.0);
        Self {
            text: mix_color(
                Color32::from_rgb(28, 36, 50),
                Color32::from_rgb(229, 234, 241),
                t,
            ),
            muted: mix_color(
                Color32::from_rgb(89, 101, 117),
                Color32::from_rgb(148, 160, 178),
                t,
            ),
            brand: mix_color(
                Color32::from_rgb(24, 35, 53),
                Color32::from_rgb(238, 242, 248),
                t,
            ),
            section: mix_color(
                Color32::from_rgb(31, 42, 68),
                Color32::from_rgb(218, 226, 238),
                t,
            ),
            panel: mix_color(
                Color32::from_rgb(246, 248, 251),
                Color32::from_rgb(20, 26, 36),
                t,
            ),
            surface: mix_color(
                Color32::from_rgb(255, 255, 255),
                Color32::from_rgb(16, 21, 30),
                t,
            ),
            faint: mix_color(
                Color32::from_rgb(235, 240, 246),
                Color32::from_rgb(27, 36, 50),
                t,
            ),
            input: mix_color(
                Color32::from_rgb(255, 255, 255),
                Color32::from_rgb(12, 17, 24),
                t,
            ),
            code_bg: mix_color(
                Color32::from_rgb(238, 242, 246),
                Color32::from_rgb(27, 34, 46),
                t,
            ),
            control_bg: mix_color(
                Color32::from_rgb(246, 249, 252),
                Color32::from_rgb(28, 37, 50),
                t,
            ),
            control_hover: mix_color(
                Color32::from_rgb(226, 234, 241),
                Color32::from_rgb(41, 53, 70),
                t,
            ),
            control_active: mix_color(
                Color32::from_rgb(44, 110, 157),
                Color32::from_rgb(82, 146, 191),
                t,
            ),
            border: mix_color(
                Color32::from_rgb(215, 222, 232),
                Color32::from_rgb(51, 63, 80),
                t,
            ),
            border_strong: mix_color(
                Color32::from_rgb(186, 197, 212),
                Color32::from_rgb(82, 97, 119),
                t,
            ),
            accent: mix_color(
                Color32::from_rgb(44, 110, 157),
                Color32::from_rgb(95, 170, 220),
                t,
            ),
            selected_fill: mix_color(
                Color32::from_rgb(218, 235, 247),
                Color32::from_rgb(35, 66, 91),
                t,
            ),
            selected_text: mix_color(
                Color32::from_rgb(17, 42, 63),
                Color32::from_rgb(242, 248, 252),
                t,
            ),
            warning: mix_color(
                Color32::from_rgb(145, 92, 24),
                Color32::from_rgb(232, 164, 76),
                t,
            ),
            success: mix_color(
                Color32::from_rgb(34, 112, 83),
                Color32::from_rgb(103, 202, 159),
                t,
            ),
            due: mix_color(
                Color32::from_rgb(122, 74, 32),
                Color32::from_rgb(219, 160, 91),
                t,
            ),
            error: mix_color(
                Color32::from_rgb(176, 48, 48),
                Color32::from_rgb(245, 116, 116),
                t,
            ),
        }
    }
}

fn configure_style(ctx: &egui::Context, dark_factor: f32) {
    let palette = Palette::at(dark_factor);
    let mut style = egui::Theme::from_dark_mode(dark_factor >= 0.5).default_style();
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 7.0);
    style.visuals = themed_visuals(palette, dark_factor);
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

fn theme_toggle(ui: &mut egui::Ui, dark_mode: &mut bool, palette: Palette) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        if theme_icon_button(ui, !*dark_mode, "☀", "Light", palette).clicked() {
            *dark_mode = false;
            ui.ctx().set_theme(egui::Theme::Light);
        }
        if theme_icon_button(ui, *dark_mode, "🌙", "Dark", palette).clicked() {
            *dark_mode = true;
            ui.ctx().set_theme(egui::Theme::Dark);
        }
    });
}

fn theme_icon_button(
    ui: &mut egui::Ui,
    selected: bool,
    icon: &'static str,
    hover_text: &'static str,
    palette: Palette,
) -> egui::Response {
    let text = RichText::new(icon).size(16.0).color(if selected {
        palette.selected_text
    } else {
        palette.text
    });
    let mut button = egui::Button::selectable(selected, text).corner_radius(14.0);
    if selected {
        button = button.fill(palette.selected_fill);
    }
    ui.add_sized([32.0, 28.0], button).on_hover_text(hover_text)
}

fn themed_visuals(palette: Palette, dark_factor: f32) -> egui::Visuals {
    let mut visuals = egui::Theme::from_dark_mode(dark_factor >= 0.5).default_visuals();
    visuals.dark_mode = dark_factor >= 0.5;
    visuals.override_text_color = Some(palette.text);
    visuals.weak_text_color = Some(palette.muted);
    visuals.panel_fill = palette.panel;
    visuals.window_fill = palette.surface;
    visuals.faint_bg_color = palette.faint;
    visuals.extreme_bg_color = palette.input;
    visuals.text_edit_bg_color = Some(palette.input);
    visuals.code_bg_color = palette.code_bg;
    visuals.hyperlink_color = palette.accent;
    visuals.warn_fg_color = palette.warning;
    visuals.error_fg_color = palette.error;
    visuals.selection.bg_fill = palette.selected_fill;
    visuals.selection.stroke = Stroke::new(1.0, palette.selected_text);
    visuals.window_stroke.color = palette.border;

    visuals.widgets.noninteractive.bg_fill = palette.panel;
    visuals.widgets.noninteractive.weak_bg_fill = palette.faint;
    visuals.widgets.noninteractive.bg_stroke.color = palette.border;
    visuals.widgets.noninteractive.fg_stroke.color = palette.text;

    visuals.widgets.inactive.bg_fill = palette.control_bg;
    visuals.widgets.inactive.weak_bg_fill = palette.control_bg;
    visuals.widgets.inactive.bg_stroke.color = palette.border;
    visuals.widgets.inactive.fg_stroke.color = palette.text;

    visuals.widgets.hovered.bg_fill = palette.control_hover;
    visuals.widgets.hovered.weak_bg_fill = palette.control_hover;
    visuals.widgets.hovered.bg_stroke.color = palette.border_strong;
    visuals.widgets.hovered.fg_stroke.color = palette.text;

    visuals.widgets.active.bg_fill = palette.control_active;
    visuals.widgets.active.weak_bg_fill = palette.control_active;
    visuals.widgets.active.bg_stroke.color = palette.accent;
    visuals.widgets.active.fg_stroke.color = palette.selected_text;
    visuals.widgets.open = visuals.widgets.hovered;
    visuals
}

fn mix_color(light: Color32, dark: Color32, factor: f32) -> Color32 {
    let factor = factor.clamp(0.0, 1.0);
    let mix = |start: u8, end: u8| {
        (start as f32 + (end as f32 - start as f32) * factor)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Color32::from_rgba_unmultiplied(
        mix(light.r(), dark.r()),
        mix(light.g(), dark.g()),
        mix(light.b(), dark.b()),
        mix(light.a(), dark.a()),
    )
}

fn section_header(ui: &mut egui::Ui, title: &str, palette: Palette) {
    ui.add_space(12.0);
    ui.heading(RichText::new(title).color(palette.section));
    ui.add_space(8.0);
}

fn task_list(ui: &mut egui::Ui, tasks: &[Task], palette: Palette) {
    if tasks.is_empty() {
        ui.label("No tasks.");
        return;
    }

    ScrollArea::vertical().show(ui, |ui| {
        for task in tasks {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&task.title).strong());
                if let Some(due_date) = task.due_date {
                    ui.label(RichText::new(format!("due {due_date}")).color(palette.due));
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
