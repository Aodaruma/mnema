use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{Result, anyhow};
use eframe::egui::{
    self, Align, Color32, FontData, FontDefinitions, FontFamily, FontTweak, RichText, ScrollArea,
    Stroke, TextEdit,
};
use mnema_app::{
    CaptureTaskRequest, CaptureTaskService, PlanTodayRequest, PlanTodayResult,
    ProposedScheduleBlock, RepairScheduleRequest, RepairScheduleService,
    ScheduleBlockCommandService, SchedulePlanStoreService, TaskCommandService,
    UpdateScheduleBlockWindowRequest, UpdateTaskRequest,
};
use mnema_core::prelude::*;
use mnema_infra::{
    db::{StorageBackend, Vault},
    llm::{ChatMessage, ChatRole, LlmClient, LlmConfig, OllamaClient, OpenAiCompatibleClient},
};
use muda::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use serde::{Deserialize, Serialize};
use time::{Date, OffsetDateTime, Time, macros::format_description};
use tokio::runtime::Runtime;

static MENU_EVENTS: OnceLock<Mutex<Vec<MenuEvent>>> = OnceLock::new();

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
    Projects,
    Schedule,
    Assistant,
    Activity,
    Settings,
}

#[derive(Debug, Clone)]
enum TaskAction {
    Edit(TaskId),
    Complete(TaskId),
    Delete(TaskId),
}

#[derive(Debug, Clone)]
enum ScheduleAction {
    Edit(ScheduleBlockId),
    SetState(ScheduleBlockId, ScheduleBlockState),
}

#[derive(Debug, Clone, Copy)]
enum MenuAction {
    Refresh,
    Close,
    SetView(View),
    SetDarkMode(bool),
}

#[derive(Debug, Clone)]
struct AssistantChatMessage {
    role: &'static str,
    content: String,
}

const INPUT_HEIGHT: f32 = 34.0;
const THEME_SWITCH_SIZE: egui::Vec2 = egui::vec2(76.0, 34.0);
const FONT_WEIGHT_REGULAR: f32 = 400.0;
const FONT_WEIGHT_BOLD: f32 = 700.0;
const LOGO_FONT_FAMILY: &str = "mnema_logo";
const DEFAULT_POSTGRES_URL: &str = "postgres://postgres:postgres@localhost/mnema";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DesktopStorageBackend {
    Sqlite,
    Postgres,
}

impl DesktopStorageBackend {
    fn from_connected(backend: StorageBackend) -> Self {
        match backend {
            StorageBackend::Sqlite => Self::Sqlite,
            StorageBackend::Postgres => Self::Postgres,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Sqlite => "SQLite",
            Self::Postgres => "PostgreSQL",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DesktopLlmProvider {
    Disabled,
    Ollama,
    OpenAiCompatible,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DesktopConfig {
    vault_path: String,
    storage_backend: DesktopStorageBackend,
    sqlite_path: String,
    database_url: String,
    dark_mode: bool,
    llm_provider: DesktopLlmProvider,
    ollama_url: String,
    openai_url: String,
    planning_model: String,
    routine_model: String,
}

impl DesktopConfig {
    fn load(initial_vault_path: PathBuf, default_dark_mode: bool) -> Self {
        let fallback = Self {
            vault_path: initial_vault_path.display().to_string(),
            storage_backend: DesktopStorageBackend::Sqlite,
            sqlite_path: default_sqlite_path().display().to_string(),
            database_url: DEFAULT_POSTGRES_URL.to_string(),
            dark_mode: default_dark_mode,
            llm_provider: DesktopLlmProvider::Disabled,
            ollama_url: "http://localhost:11434".to_string(),
            openai_url: "https://api.openai.com".to_string(),
            planning_model: "gpt-4.1".to_string(),
            routine_model: "gpt-4.1-mini".to_string(),
        };

        let Ok(bytes) = fs::read(config_path()) else {
            return fallback;
        };

        serde_json::from_slice::<Self>(&bytes).unwrap_or(fallback)
    }
}

struct NativeMenu {
    root: Menu,
    refresh: MenuItem,
    close: MenuItem,
    today: MenuItem,
    inbox: MenuItem,
    projects: MenuItem,
    schedule: MenuItem,
    assistant: MenuItem,
    activity: MenuItem,
    settings: MenuItem,
    light_mode: CheckMenuItem,
    dark_mode: CheckMenuItem,
    #[cfg(target_os = "windows")]
    hwnd: Option<isize>,
}

impl NativeMenu {
    fn install(cc: &eframe::CreationContext<'_>, dark_mode: bool) -> Result<Self> {
        install_menu_event_handler(&cc.egui_ctx);

        let mut menu = Self::new(dark_mode)?;
        menu.attach(cc, dark_mode)?;
        Ok(menu)
    }

    fn new(dark_mode: bool) -> Result<Self> {
        let refresh = MenuItem::with_id("mnema.file.refresh", "&Refresh", true, None);
        let close = MenuItem::with_id("mnema.file.close", "&Close", true, None);
        let file_separator = PredefinedMenuItem::separator();
        let file_menu = Submenu::with_items("&File", true, &[&refresh, &file_separator, &close])?;

        let undo = PredefinedMenuItem::undo(Some("&Undo"));
        let redo = PredefinedMenuItem::redo(Some("&Redo"));
        let cut = PredefinedMenuItem::cut(Some("Cu&t"));
        let copy = PredefinedMenuItem::copy(Some("&Copy"));
        let paste = PredefinedMenuItem::paste(Some("&Paste"));
        let select_all = PredefinedMenuItem::select_all(Some("Select &All"));
        let edit_separator_a = PredefinedMenuItem::separator();
        let edit_separator_b = PredefinedMenuItem::separator();
        let edit_menu = Submenu::with_items(
            "&Edit",
            true,
            &[
                &undo,
                &redo,
                &edit_separator_a,
                &cut,
                &copy,
                &paste,
                &edit_separator_b,
                &select_all,
            ],
        )?;

        let today = MenuItem::with_id("mnema.view.today", "&Today", true, None);
        let inbox = MenuItem::with_id("mnema.view.inbox", "&Inbox", true, None);
        let projects = MenuItem::with_id("mnema.view.projects", "&Projects", true, None);
        let schedule = MenuItem::with_id("mnema.view.schedule", "&Schedule", true, None);
        let assistant = MenuItem::with_id("mnema.view.assistant", "&Assistant", true, None);
        let activity = MenuItem::with_id("mnema.view.activity", "Acti&vity", true, None);
        let settings = MenuItem::with_id("mnema.view.settings", "Se&ttings", true, None);
        let light_mode =
            CheckMenuItem::with_id("mnema.theme.light", "&Light mode", true, !dark_mode, None);
        let dark_mode_item =
            CheckMenuItem::with_id("mnema.theme.dark", "&Dark mode", true, dark_mode, None);
        let view_separator = PredefinedMenuItem::separator();
        let view_menu = Submenu::with_items(
            "&View",
            true,
            &[
                &today,
                &inbox,
                &projects,
                &schedule,
                &assistant,
                &activity,
                &settings,
                &view_separator,
                &light_mode,
                &dark_mode_item,
            ],
        )?;

        let about = MenuItem::with_id("mnema.help.about", "&About Mnema", false, None);
        let help_menu = Submenu::with_items("&Help", true, &[&about])?;
        let root = Menu::with_items(&[&file_menu, &edit_menu, &view_menu, &help_menu])?;

        Ok(Self {
            root,
            refresh,
            close,
            today,
            inbox,
            projects,
            schedule,
            assistant,
            activity,
            settings,
            light_mode,
            dark_mode: dark_mode_item,
            #[cfg(target_os = "windows")]
            hwnd: None,
        })
    }

    fn attach(&mut self, cc: &eframe::CreationContext<'_>, dark_mode: bool) -> Result<()> {
        #[cfg(target_os = "windows")]
        {
            let hwnd = hwnd_from_creation_context(cc)?;
            unsafe {
                self.root
                    .init_for_hwnd_with_theme(hwnd, menu_theme(dark_mode))?;
            }
            self.hwnd = Some(hwnd);
        }

        #[cfg(target_os = "macos")]
        {
            let _ = cc;
            let _ = dark_mode;
            self.root.init_for_nsapp();
        }

        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            let _ = cc;
            let _ = dark_mode;
        }

        Ok(())
    }

    fn sync_theme(&self, dark_mode: bool) {
        self.light_mode.set_checked(!dark_mode);
        self.dark_mode.set_checked(dark_mode);

        #[cfg(target_os = "windows")]
        if let Some(hwnd) = self.hwnd {
            let _ = unsafe { self.root.set_theme_for_hwnd(hwnd, menu_theme(dark_mode)) };
        }
    }

    fn action_for(&self, event: &MenuEvent) -> Option<MenuAction> {
        let id = event.id().as_ref();
        if id == self.refresh.id().as_ref() {
            Some(MenuAction::Refresh)
        } else if id == self.close.id().as_ref() {
            Some(MenuAction::Close)
        } else if id == self.today.id().as_ref() {
            Some(MenuAction::SetView(View::Today))
        } else if id == self.inbox.id().as_ref() {
            Some(MenuAction::SetView(View::Inbox))
        } else if id == self.projects.id().as_ref() {
            Some(MenuAction::SetView(View::Projects))
        } else if id == self.schedule.id().as_ref() {
            Some(MenuAction::SetView(View::Schedule))
        } else if id == self.assistant.id().as_ref() {
            Some(MenuAction::SetView(View::Assistant))
        } else if id == self.activity.id().as_ref() {
            Some(MenuAction::SetView(View::Activity))
        } else if id == self.settings.id().as_ref() {
            Some(MenuAction::SetView(View::Settings))
        } else if id == self.light_mode.id().as_ref() {
            Some(MenuAction::SetDarkMode(false))
        } else if id == self.dark_mode.id().as_ref() {
            Some(MenuAction::SetDarkMode(true))
        } else {
            None
        }
    }
}

fn menu_event_queue() -> &'static Mutex<Vec<MenuEvent>> {
    MENU_EVENTS.get_or_init(|| Mutex::new(Vec::new()))
}

fn install_menu_event_handler(ctx: &egui::Context) {
    let ctx = ctx.clone();
    MenuEvent::set_event_handler(Some(move |event| {
        if let Ok(mut events) = menu_event_queue().lock() {
            events.push(event);
        }
        ctx.request_repaint();
    }));
}

fn drain_menu_events() -> Vec<MenuEvent> {
    let Ok(mut events) = menu_event_queue().lock() else {
        return Vec::new();
    };
    events.drain(..).collect()
}

#[cfg(target_os = "windows")]
fn hwnd_from_creation_context(cc: &eframe::CreationContext<'_>) -> Result<isize> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let handle = cc
        .window_handle()
        .map_err(|error| anyhow!("window handle unavailable: {error}"))?
        .as_raw();
    match handle {
        RawWindowHandle::Win32(handle) => Ok(handle.hwnd.get()),
        _ => Err(anyhow!("unsupported native window handle for menu")),
    }
}

#[cfg(target_os = "windows")]
fn menu_theme(dark_mode: bool) -> muda::MenuTheme {
    if dark_mode {
        muda::MenuTheme::Dark
    } else {
        muda::MenuTheme::Light
    }
}

struct MnemaGuiApp {
    runtime: Runtime,
    vault_path: String,
    vault: Option<Vault>,
    backend: Option<StorageBackend>,
    selected_backend: DesktopStorageBackend,
    sqlite_path: String,
    database_url: String,
    llm_provider: DesktopLlmProvider,
    ollama_url: String,
    openai_url: String,
    planning_model: String,
    routine_model: String,
    view: View,
    task_title: String,
    quick_capture: String,
    assistant_input: String,
    assistant_messages: Vec<AssistantChatMessage>,
    due_date: String,
    minutes: String,
    editing_task_id: Option<TaskId>,
    edit_title: String,
    edit_due_date: String,
    edit_minutes: String,
    editing_schedule_block_id: Option<ScheduleBlockId>,
    schedule_edit_start: String,
    schedule_edit_end: String,
    confirm_plan_save: bool,
    confirm_repair_save: bool,
    target_date: String,
    repair_from_time: String,
    tasks: Vec<Task>,
    projects: Vec<Project>,
    selected_project_id: Option<ProjectId>,
    project_lists: Vec<List>,
    milestones: Vec<Milestone>,
    project_title: String,
    project_start_date: String,
    project_end_date: String,
    project_list_name: String,
    milestone_title: String,
    milestone_target_date: String,
    plan: Option<PlanTodayResult>,
    schedule: Vec<ScheduleBlock>,
    automation_logs: Vec<AutomationLog>,
    message: String,
    error: Option<String>,
    dark_mode: bool,
    native_menu: Option<NativeMenu>,
    native_menu_synced_dark_mode: Option<bool>,
    settings_message: String,
}

impl MnemaGuiApp {
    fn new(cc: &eframe::CreationContext<'_>, initial_vault_path: PathBuf) -> Self {
        configure_fonts(&cc.egui_ctx);
        let default_dark_mode = cc.egui_ctx.theme() == egui::Theme::Dark;
        let config = DesktopConfig::load(initial_vault_path, default_dark_mode);
        let dark_mode = config.dark_mode;
        configure_style(&cc.egui_ctx, if dark_mode { 1.0 } else { 0.0 }, dark_mode);
        let native_menu = match NativeMenu::install(cc, dark_mode) {
            Ok(menu) => Some(menu),
            Err(error) => {
                tracing::warn!(?error, "failed to install native menu");
                None
            }
        };
        let native_menu_synced_dark_mode = native_menu.as_ref().map(|_| dark_mode);

        let today = OffsetDateTime::now_utc().date().to_string();
        let current_time = format_hm(OffsetDateTime::now_utc());
        let runtime = Runtime::new().expect("tokio runtime must initialize for Mnema GUI");
        let mut app = Self {
            runtime,
            vault_path: config.vault_path,
            vault: None,
            backend: None,
            selected_backend: config.storage_backend,
            sqlite_path: config.sqlite_path,
            database_url: config.database_url,
            llm_provider: config.llm_provider,
            ollama_url: config.ollama_url,
            openai_url: config.openai_url,
            planning_model: config.planning_model,
            routine_model: config.routine_model,
            view: View::Today,
            task_title: String::new(),
            quick_capture: String::new(),
            assistant_input: String::new(),
            assistant_messages: vec![AssistantChatMessage {
                role: "Assistant",
                content:
                    "タスク作成や今日の次アクションを相談できます。例: Write proposal tomorrow 45m"
                        .into(),
            }],
            due_date: today.clone(),
            minutes: String::from("45"),
            editing_task_id: None,
            edit_title: String::new(),
            edit_due_date: String::new(),
            edit_minutes: String::new(),
            editing_schedule_block_id: None,
            schedule_edit_start: String::new(),
            schedule_edit_end: String::new(),
            confirm_plan_save: false,
            confirm_repair_save: false,
            target_date: today.clone(),
            repair_from_time: current_time,
            tasks: Vec::new(),
            projects: Vec::new(),
            selected_project_id: None,
            project_lists: Vec::new(),
            milestones: Vec::new(),
            project_title: String::new(),
            project_start_date: today.clone(),
            project_end_date: String::new(),
            project_list_name: String::new(),
            milestone_title: String::new(),
            milestone_target_date: today.clone(),
            plan: None,
            schedule: Vec::new(),
            automation_logs: Vec::new(),
            message: String::new(),
            error: None,
            dark_mode,
            native_menu,
            native_menu_synced_dark_mode,
            settings_message: String::new(),
        };
        app.connect_and_refresh();
        app
    }

    fn connect_and_refresh(&mut self) {
        let path = self.normalized_vault_path();
        let selected_backend = self.selected_backend;
        let sqlite_path = self.normalized_sqlite_path();
        let database_url = self.normalized_database_url();
        let result = self.runtime.block_on(async move {
            let vault = match selected_backend {
                DesktopStorageBackend::Sqlite => {
                    Vault::connect_or_init_with_sqlite_path(&path, sqlite_path).await?
                }
                DesktopStorageBackend::Postgres => {
                    Vault::connect_or_init_with_database_url(&path, &database_url).await?
                }
            };
            vault.initialize_defaults().await?;
            Result::<Vault>::Ok(vault)
        });

        match result {
            Ok(vault) => {
                self.backend = Some(vault.backend());
                self.selected_backend = DesktopStorageBackend::from_connected(vault.backend());
                self.vault = Some(vault);
                self.message = String::from("Vault connected");
                self.settings_message = String::from("Connected");
                self.error = None;
                self.refresh_tasks();
                self.refresh_projects();
                self.refresh_schedule();
                self.refresh_automation_logs();
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
            let status_repo = vault.status_repo();
            let mut tasks = task_repo.list_all().await?;
            let done_status_ids = done_status_ids(status_repo.as_ref(), &tasks).await?;
            tasks.retain(|task| {
                task.deleted_at.is_none() && !done_status_ids.contains(&task.status_id)
            });
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

    fn refresh_projects(&mut self) {
        let Ok(vault) = self.vault_clone() else {
            return;
        };
        let result = self.runtime.block_on(async move {
            let project_repo = vault.project_repo();
            let mut projects = project_repo.list_all().await?;
            projects.sort_by(|left, right| left.title.cmp(&right.title));
            Result::<Vec<Project>>::Ok(projects)
        });

        match result {
            Ok(projects) => {
                if let Some(selected) = &self.selected_project_id {
                    if !projects.iter().any(|project| &project.id == selected) {
                        self.selected_project_id = None;
                        self.project_lists.clear();
                        self.milestones.clear();
                    }
                }
                self.projects = projects;
                self.error = None;
                self.refresh_project_children();
            }
            Err(error) => self.set_error(error),
        }
    }

    fn refresh_project_children(&mut self) {
        let Some(project_id) = self.selected_project_id.clone() else {
            self.project_lists.clear();
            self.milestones.clear();
            return;
        };
        let Ok(vault) = self.vault_clone() else {
            return;
        };
        let result = self.runtime.block_on(async move {
            let lists = vault
                .list_repo()
                .list_by_project(project_id.clone())
                .await?;
            let milestones = vault.milestone_repo().list_by_project(project_id).await?;
            Result::<(Vec<List>, Vec<Milestone>)>::Ok((lists, milestones))
        });

        match result {
            Ok((mut lists, mut milestones)) => {
                lists.sort_by_key(|list| list.order);
                milestones.sort_by_key(|milestone| milestone.target_date);
                self.project_lists = lists;
                self.milestones = milestones;
                self.error = None;
            }
            Err(error) => self.set_error(error),
        }
    }

    fn complete_task(&mut self, task_id: TaskId) {
        let Ok(vault) = self.vault_clone() else {
            return;
        };
        let completed_task_id = task_id.clone();
        let result = self.runtime.block_on(async move {
            let task_repo = vault.task_repo();
            let status_repo = vault.status_repo();
            let service = TaskCommandService::new(task_repo.as_ref(), status_repo.as_ref());
            Result::<Task>::Ok(service.complete_task(task_id).await?)
        });

        match result {
            Ok(task) => {
                self.message = format!("Completed: {}", task.title);
                self.error = None;
                if self.editing_task_id.as_ref() == Some(&completed_task_id) {
                    self.clear_task_editor();
                }
                self.refresh_tasks();
                self.refresh_schedule();
            }
            Err(error) => self.set_error(error),
        }
    }

    fn delete_task(&mut self, task_id: TaskId) {
        let Ok(vault) = self.vault_clone() else {
            return;
        };
        let deleted_task_id = task_id.clone();
        let result = self.runtime.block_on(async move {
            let task_repo = vault.task_repo();
            let status_repo = vault.status_repo();
            let service = TaskCommandService::new(task_repo.as_ref(), status_repo.as_ref());
            service.delete_task(task_id).await?;
            Result::<()>::Ok(())
        });

        match result {
            Ok(()) => {
                self.message = String::from("Task deleted");
                self.error = None;
                if self.editing_task_id.as_ref() == Some(&deleted_task_id) {
                    self.clear_task_editor();
                }
                self.refresh_tasks();
                self.refresh_schedule();
            }
            Err(error) => self.set_error(error),
        }
    }

    fn handle_task_action(&mut self, action: TaskAction) {
        match action {
            TaskAction::Edit(task_id) => self.start_edit_task(task_id),
            TaskAction::Complete(task_id) => self.complete_task(task_id),
            TaskAction::Delete(task_id) => self.delete_task(task_id),
        }
    }

    fn start_edit_task(&mut self, task_id: TaskId) {
        let Some(task) = self.tasks.iter().find(|task| task.id == task_id) else {
            self.set_error(anyhow!("task not found"));
            return;
        };
        self.editing_task_id = Some(task.id.clone());
        self.edit_title = task.title.clone();
        self.edit_due_date = task
            .due_date
            .map(|date| date.to_string())
            .unwrap_or_default();
        self.edit_minutes = task
            .estimated_minutes
            .map(|minutes| minutes.to_string())
            .unwrap_or_default();
        self.error = None;
    }

    fn save_task_edit(&mut self) {
        let Some(task_id) = self.editing_task_id.clone() else {
            return;
        };
        let title = self.edit_title.trim().to_string();
        if title.is_empty() {
            self.set_error(anyhow!("タスク名を入力してください"));
            return;
        }
        let due_date = match parse_optional_date(&self.edit_due_date) {
            Ok(date) => date,
            Err(error) => {
                self.set_error(error);
                return;
            }
        };
        let estimated_minutes = match parse_optional_u32(&self.edit_minutes) {
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
            let task_repo = vault.task_repo();
            let status_repo = vault.status_repo();
            let service = TaskCommandService::new(task_repo.as_ref(), status_repo.as_ref());
            Result::<Task>::Ok(
                service
                    .update_task(UpdateTaskRequest {
                        task_id,
                        title,
                        due_date,
                        estimated_minutes,
                    })
                    .await?,
            )
        });

        match result {
            Ok(task) => {
                self.message = format!("Updated: {}", task.title);
                self.error = None;
                self.clear_task_editor();
                self.refresh_tasks();
                self.refresh_schedule();
            }
            Err(error) => self.set_error(error),
        }
    }

    fn clear_task_editor(&mut self) {
        self.editing_task_id = None;
        self.edit_title.clear();
        self.edit_due_date.clear();
        self.edit_minutes.clear();
    }

    fn send_assistant_message(&mut self) {
        let input = self.assistant_input.trim().to_string();
        if input.is_empty() {
            return;
        }

        self.assistant_messages.push(AssistantChatMessage {
            role: "You",
            content: input.clone(),
        });
        self.assistant_input.clear();

        if asks_next_action(&input) {
            let response = self
                .assistant_llm_response(&input)
                .unwrap_or_else(|| self.next_action_response());
            self.assistant_messages.push(AssistantChatMessage {
                role: "Assistant",
                content: response,
            });
            return;
        }

        match parse_quick_capture(&input, OffsetDateTime::now_utc().date()) {
            Ok(request) => {
                let title = request.title.clone();
                self.capture_task(request);
                if self.error.is_none() {
                    self.assistant_messages.push(AssistantChatMessage {
                        role: "Assistant",
                        content: format!("タスクを作成しました: {title}"),
                    });
                }
            }
            Err(_) => {
                let response = self.assistant_llm_response(&input).unwrap_or_else(|| {
                    "今はタスク作成と次アクション相談に対応しています。例: Write proposal tomorrow 45m"
                        .into()
                });
                self.assistant_messages.push(AssistantChatMessage {
                    role: "Assistant",
                    content: response,
                });
            }
        }
    }

    fn assistant_llm_response(&mut self, input: &str) -> Option<String> {
        if self.llm_provider == DesktopLlmProvider::Disabled {
            return None;
        }

        let provider = self.llm_provider;
        let ollama_url = self.ollama_url.trim().to_string();
        let openai_url = self.openai_url.trim().to_string();
        let model = match provider {
            DesktopLlmProvider::Disabled => return None,
            DesktopLlmProvider::Ollama => non_empty_or(&self.routine_model, "llama3.1"),
            DesktopLlmProvider::OpenAiCompatible => {
                non_empty_or(&self.routine_model, "gpt-4.1-mini")
            }
        };
        let context = self.assistant_context();
        let user_input = input.to_string();

        let result = self.runtime.block_on(async move {
            let config = LlmConfig {
                model,
                temperature: Some(0.3),
                max_tokens: Some(500),
            };
            let messages = vec![
                ChatMessage {
                    role: ChatRole::System,
                    content: "You are Mnema's concise Japanese task assistant. Help the user decide next actions and keep responses practical. Do not invent saved data.".into(),
                },
                ChatMessage {
                    role: ChatRole::User,
                    content: format!("Current Mnema context:\n{context}\n\nUser message:\n{user_input}"),
                },
            ];

            match provider {
                DesktopLlmProvider::Disabled => unreachable!(),
                DesktopLlmProvider::Ollama => {
                    let client = OllamaClient::new(ollama_url);
                    client.chat(&messages, &config).await
                }
                DesktopLlmProvider::OpenAiCompatible => {
                    let api_key = std::env::var("MNEMA_OPENAI_API_KEY")
                        .or_else(|_| std::env::var("OPENAI_API_KEY"))
                        .unwrap_or_default();
                    let client = OpenAiCompatibleClient::new(openai_url, api_key);
                    client.chat(&messages, &config).await
                }
            }
        });

        Some(match result {
            Ok(response) => response,
            Err(error) => format!("LLM 呼び出しに失敗しました: {error}"),
        })
    }

    fn assistant_context(&self) -> String {
        let tasks = self
            .tasks
            .iter()
            .take(10)
            .map(|task| {
                let mut fields = Vec::new();
                if let Some(due_date) = task.due_date {
                    fields.push(format!("due {due_date}"));
                }
                if let Some(minutes) = task.estimated_minutes {
                    fields.push(format!("{minutes}m"));
                }
                if fields.is_empty() {
                    format!("- {}", task.title)
                } else {
                    format!("- {} ({})", task.title, fields.join(", "))
                }
            })
            .collect::<Vec<_>>();
        let schedule = self
            .schedule
            .iter()
            .take(10)
            .map(|block| {
                format!(
                    "- {}-{} {} [{}]",
                    format_hm(block.start_at),
                    format_hm(block.end_at),
                    block
                        .title_snapshot
                        .as_deref()
                        .unwrap_or("(untitled block)"),
                    schedule_state_label(&block.state)
                )
            })
            .collect::<Vec<_>>();

        format!(
            "Tasks:\n{}\n\nSchedule:\n{}",
            if tasks.is_empty() {
                "- none".to_string()
            } else {
                tasks.join("\n")
            },
            if schedule.is_empty() {
                "- none".to_string()
            } else {
                schedule.join("\n")
            }
        )
    }

    fn next_action_response(&self) -> String {
        let Some(task) = self.tasks.first() else {
            return "今のところ未完了タスクはありません。Inbox に気になることを追加しておくと計画できます。".into();
        };

        let mut details = Vec::new();
        if let Some(due_date) = task.due_date {
            details.push(format!("due {due_date}"));
        }
        if let Some(minutes) = task.estimated_minutes {
            details.push(format!("{minutes}m"));
        }
        let suffix = if details.is_empty() {
            String::new()
        } else {
            format!(" ({})", details.join(", "))
        };
        format!(
            "次は「{}{}」から進めるのがよさそうです。",
            task.title, suffix
        )
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

    fn refresh_automation_logs(&mut self) {
        let Ok(vault) = self.vault_clone() else {
            return;
        };
        let result = self.runtime.block_on(async move {
            let repo = vault.automation_log_repo();
            Result::<Vec<AutomationLog>>::Ok(repo.list_recent(100).await?)
        });

        match result {
            Ok(logs) => {
                self.automation_logs = logs;
                self.error = None;
            }
            Err(error) => self.set_error(error),
        }
    }

    fn update_schedule_block_state(
        &mut self,
        block_id: ScheduleBlockId,
        state: ScheduleBlockState,
    ) {
        let Ok(vault) = self.vault_clone() else {
            return;
        };
        let result = self.runtime.block_on(async move {
            let schedule_block_repo = vault.schedule_block_repo();
            let service = ScheduleBlockCommandService::new(schedule_block_repo.as_ref());
            Result::<ScheduleBlock>::Ok(service.update_state(block_id, state).await?)
        });

        match result {
            Ok(block) => {
                self.message = format!(
                    "Updated block: {}",
                    block
                        .title_snapshot
                        .as_deref()
                        .unwrap_or("(untitled block)")
                );
                self.error = None;
                self.refresh_schedule();
            }
            Err(error) => self.set_error(error),
        }
    }

    fn start_edit_schedule_block(&mut self, block_id: ScheduleBlockId) {
        let Some(block) = self.schedule.iter().find(|block| block.id == block_id) else {
            self.set_error(anyhow!("schedule block not found"));
            return;
        };
        self.editing_schedule_block_id = Some(block.id.clone());
        self.schedule_edit_start = format_hm(block.start_at);
        self.schedule_edit_end = format_hm(block.end_at);
        self.error = None;
    }

    fn save_schedule_block_edit(&mut self) {
        let Some(block_id) = self.editing_schedule_block_id.clone() else {
            return;
        };
        let target_date = match parse_required_date(&self.target_date) {
            Ok(date) => date,
            Err(error) => {
                self.set_error(error);
                return;
            }
        };
        let start_at = match parse_hm_for_date(&self.schedule_edit_start, target_date) {
            Ok(value) => value,
            Err(error) => {
                self.set_error(error);
                return;
            }
        };
        let end_at = match parse_hm_for_date(&self.schedule_edit_end, target_date) {
            Ok(value) => value,
            Err(error) => {
                self.set_error(error);
                return;
            }
        };
        let Ok(vault) = self.vault_clone() else {
            return;
        };

        let result = self.runtime.block_on(async move {
            let schedule_block_repo = vault.schedule_block_repo();
            let service = ScheduleBlockCommandService::new(schedule_block_repo.as_ref());
            Result::<ScheduleBlock>::Ok(
                service
                    .update_window(UpdateScheduleBlockWindowRequest {
                        block_id,
                        start_at,
                        end_at,
                    })
                    .await?,
            )
        });

        match result {
            Ok(block) => {
                self.message = format!(
                    "Updated time: {}",
                    block
                        .title_snapshot
                        .as_deref()
                        .unwrap_or("(untitled block)")
                );
                self.error = None;
                self.clear_schedule_block_editor();
                self.refresh_schedule();
            }
            Err(error) => self.set_error(error),
        }
    }

    fn clear_schedule_block_editor(&mut self) {
        self.editing_schedule_block_id = None;
        self.schedule_edit_start.clear();
        self.schedule_edit_end.clear();
    }

    fn handle_schedule_action(&mut self, action: ScheduleAction) {
        match action {
            ScheduleAction::Edit(block_id) => self.start_edit_schedule_block(block_id),
            ScheduleAction::SetState(block_id, state) => {
                self.update_schedule_block_state(block_id, state)
            }
        }
    }

    fn add_task(&mut self) {
        let title = self.task_title.trim().to_string();
        if title.is_empty() {
            self.set_error(anyhow!("タスク名を入力してください"));
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

        self.capture_task(CaptureTaskRequest {
            title,
            description: None,
            due_date,
            estimated_minutes,
        });
    }

    fn capture_quick_task(&mut self) {
        let today = OffsetDateTime::now_utc().date();
        let request = match parse_quick_capture(&self.quick_capture, today) {
            Ok(request) => request,
            Err(error) => {
                self.set_error(error);
                return;
            }
        };
        self.capture_task(request);
        if self.error.is_none() {
            self.quick_capture.clear();
        }
    }

    fn capture_task(&mut self, request: CaptureTaskRequest) {
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
            let task = service.capture_inbox_task(request).await?.task;
            let automation_log_repo = vault.automation_log_repo();
            automation_log_repo
                .insert(AutomationLog {
                    id: AutomationLogId::new(),
                    task_id: Some(task.id.clone()),
                    project_id: task.project_id.clone(),
                    list_id: task.list_id.clone(),
                    assistant_id: AssistantId::new(),
                    action_type: AutomationActionType::CreateTask,
                    before_state: None,
                    after_state: Some(serde_json::to_value(&task)?),
                    created_at: OffsetDateTime::now_utc(),
                    explanation: Some("Created from desktop capture".into()),
                })
                .await?;
            Result::<Task>::Ok(task)
        });

        match result {
            Ok(task) => {
                self.task_title.clear();
                self.message = format!("Added: {}", task.title);
                self.error = None;
                self.refresh_tasks();
                self.refresh_automation_logs();
            }
            Err(error) => self.set_error(error),
        }
    }

    fn add_project(&mut self) {
        let title = self.project_title.trim().to_string();
        if title.is_empty() {
            self.set_error(anyhow!("プロジェクト名を入力してください"));
            return;
        }
        let start_date = match parse_optional_date(&self.project_start_date) {
            Ok(date) => date,
            Err(error) => {
                self.set_error(error);
                return;
            }
        };
        let end_date = match parse_optional_date(&self.project_end_date) {
            Ok(date) => date,
            Err(error) => {
                self.set_error(error);
                return;
            }
        };
        let Ok(vault) = self.vault_clone() else {
            return;
        };
        let project = Project {
            id: ProjectId::new(),
            title,
            description: None,
            start_date,
            end_date,
            default_status_set_id: None,
            archived_at: None,
        };
        let project_id = project.id.clone();
        let result = self.runtime.block_on(async move {
            vault.project_repo().insert(project.clone()).await?;
            Result::<Project>::Ok(project)
        });

        match result {
            Ok(project) => {
                self.message = format!("Project added: {}", project.title);
                self.project_title.clear();
                self.project_end_date.clear();
                self.selected_project_id = Some(project_id);
                self.refresh_projects();
            }
            Err(error) => self.set_error(error),
        }
    }

    fn add_project_list(&mut self) {
        let Some(project_id) = self.selected_project_id.clone() else {
            self.set_error(anyhow!("プロジェクトを選択してください"));
            return;
        };
        let name = self.project_list_name.trim().to_string();
        if name.is_empty() {
            self.set_error(anyhow!("リスト名を入力してください"));
            return;
        }
        let Ok(vault) = self.vault_clone() else {
            return;
        };
        let order = self.project_lists.len() as i32;
        let list = List {
            id: ListId::new(),
            project_id: Some(project_id),
            name,
            is_system: false,
            kind: ListKind::Project,
            view_type: ListViewType::List,
            order,
        };
        let result = self.runtime.block_on(async move {
            vault.list_repo().insert(list.clone()).await?;
            Result::<List>::Ok(list)
        });

        match result {
            Ok(list) => {
                self.message = format!("List added: {}", list.name);
                self.project_list_name.clear();
                self.refresh_project_children();
            }
            Err(error) => self.set_error(error),
        }
    }

    fn add_milestone(&mut self) {
        let Some(project_id) = self.selected_project_id.clone() else {
            self.set_error(anyhow!("プロジェクトを選択してください"));
            return;
        };
        let title = self.milestone_title.trim().to_string();
        if title.is_empty() {
            self.set_error(anyhow!("マイルストーン名を入力してください"));
            return;
        }
        let target_date = match parse_required_date(&self.milestone_target_date) {
            Ok(date) => date,
            Err(error) => {
                self.set_error(error);
                return;
            }
        };
        let Ok(vault) = self.vault_clone() else {
            return;
        };
        let now = OffsetDateTime::now_utc();
        let milestone = Milestone {
            id: MilestoneId::new(),
            project_id,
            title,
            description: None,
            target_date,
            status: MilestoneStatus::NotDone,
            dependency_task_ids: Vec::new(),
            created_at: now,
            updated_at: now,
        };
        let result = self.runtime.block_on(async move {
            vault.milestone_repo().insert(milestone.clone()).await?;
            Result::<Milestone>::Ok(milestone)
        });

        match result {
            Ok(milestone) => {
                self.message = format!("Milestone added: {}", milestone.title);
                self.milestone_title.clear();
                self.refresh_project_children();
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

    fn request_save_plan(&mut self) {
        self.refresh_schedule();
        let has_replaceable_proposed = self
            .schedule
            .iter()
            .any(|block| block.state == ScheduleBlockState::Proposed);
        if has_replaceable_proposed {
            self.confirm_plan_save = true;
            self.confirm_repair_save = false;
            self.message = String::from("Save will replace existing proposed blocks.");
        } else {
            self.plan_today(true);
        }
    }

    fn repair_schedule(&mut self, save: bool) {
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
        let repair_from = match parse_hm_for_date(&self.repair_from_time, target_date) {
            Ok(value) => value,
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
            let schedule_block_repo = vault.schedule_block_repo();
            let service = RepairScheduleService::new(
                task_repo.as_ref(),
                status_repo.as_ref(),
                schedule_block_repo.as_ref(),
            );
            let repair = service
                .repair_day(RepairScheduleRequest {
                    target_date,
                    repair_from,
                    availability,
                })
                .await?;

            let saved = if save {
                let store = SchedulePlanStoreService::new(schedule_block_repo.as_ref());
                store.save_proposed_plan(&repair.plan).await?.len()
            } else {
                0
            };

            Result::<(mnema_app::RepairScheduleResult, usize)>::Ok((repair, saved))
        });

        match result {
            Ok((repair, saved)) => {
                let block_count = repair.plan.output.blocks.len();
                let fixed_count = repair.fixed_blocks.len();
                self.plan = Some(repair.plan);
                self.message = if save {
                    format!("Saved repaired plan: {saved} proposed blocks, {fixed_count} fixed")
                } else {
                    format!("Repaired {block_count} blocks, kept {fixed_count} fixed")
                };
                self.error = None;
                if save {
                    self.refresh_schedule();
                }
            }
            Err(error) => self.set_error(error),
        }
    }

    fn request_save_repair(&mut self) {
        self.refresh_schedule();
        let has_replaceable_proposed = self
            .schedule
            .iter()
            .any(|block| block.state == ScheduleBlockState::Proposed);
        if has_replaceable_proposed {
            self.confirm_repair_save = true;
            self.confirm_plan_save = false;
            self.message = String::from("Repair save will replace existing proposed blocks.");
        } else {
            self.repair_schedule(true);
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

    fn normalized_sqlite_path(&self) -> PathBuf {
        let trimmed = self.sqlite_path.trim();
        if trimmed.is_empty() {
            default_sqlite_path()
        } else {
            PathBuf::from(trimmed)
        }
    }

    fn normalized_database_url(&self) -> String {
        let trimmed = self.database_url.trim();
        if trimmed.is_empty() {
            DEFAULT_POSTGRES_URL.to_string()
        } else {
            trimmed.to_string()
        }
    }

    fn selected_project_title(&self) -> Option<String> {
        let selected_id = self.selected_project_id.as_ref()?;
        self.projects
            .iter()
            .find(|project| &project.id == selected_id)
            .map(|project| project.title.clone())
    }

    fn save_settings(&mut self) {
        let config = DesktopConfig {
            vault_path: self.normalized_vault_path().display().to_string(),
            storage_backend: self.selected_backend,
            sqlite_path: self.normalized_sqlite_path().display().to_string(),
            database_url: self.normalized_database_url(),
            dark_mode: self.dark_mode,
            llm_provider: self.llm_provider,
            ollama_url: self.ollama_url.trim().to_string(),
            openai_url: self.openai_url.trim().to_string(),
            planning_model: self.planning_model.trim().to_string(),
            routine_model: self.routine_model.trim().to_string(),
        };

        match save_config(&config) {
            Ok(()) => {
                self.vault_path = config.vault_path;
                self.sqlite_path = config.sqlite_path;
                self.database_url = config.database_url;
                self.ollama_url = config.ollama_url;
                self.openai_url = config.openai_url;
                self.planning_model = config.planning_model;
                self.routine_model = config.routine_model;
                self.settings_message = String::from("Settings saved");
                self.error = None;
            }
            Err(error) => self.set_error(error),
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
        self.error = Some(localized_error_message(&error));
    }

    fn handle_native_menu(&mut self, ctx: &egui::Context) {
        let actions = self
            .native_menu
            .as_ref()
            .map(|menu| {
                drain_menu_events()
                    .iter()
                    .filter_map(|event| menu.action_for(event))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        for action in actions {
            self.handle_menu_action(action, ctx);
        }

        self.sync_native_menu_theme();
    }

    fn handle_menu_action(&mut self, action: MenuAction, ctx: &egui::Context) {
        match action {
            MenuAction::Refresh => {
                self.refresh_tasks();
                self.refresh_schedule();
            }
            MenuAction::Close => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            MenuAction::SetView(view) => self.view = view,
            MenuAction::SetDarkMode(dark_mode) => self.dark_mode = dark_mode,
        }
    }

    fn sync_native_menu_theme(&mut self) {
        if self.native_menu_synced_dark_mode == Some(self.dark_mode) {
            return;
        }

        if let Some(menu) = &self.native_menu {
            menu.sync_theme(self.dark_mode);
            self.native_menu_synced_dark_mode = Some(self.dark_mode);
        }
    }
}

impl eframe::App for MnemaGuiApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.handle_native_menu(ui.ctx());

        let dark_factor = ui.ctx().animate_bool_with_time(
            egui::Id::new("mnema_theme_transition"),
            self.dark_mode,
            0.28,
        );
        configure_style(ui.ctx(), dark_factor, self.dark_mode);
        if (0.0..1.0).contains(&dark_factor) {
            ui.ctx().request_repaint();
        }
        let palette = Palette::at(dark_factor);

        egui::Panel::top("top_bar").show_inside(ui, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.heading(logo_text("Mnema").color(palette.brand));
                ui.add_space(12.0);
                ui.label(format!("Vault: {}", self.normalized_vault_path().display()));
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    theme_toggle(ui, &mut self.dark_mode, palette);
                    if ui.button("Refresh").clicked() {
                        self.refresh_tasks();
                        self.refresh_projects();
                        self.refresh_schedule();
                        self.refresh_automation_logs();
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
        self.sync_native_menu_theme();

        egui::Panel::left("navigation")
            .resizable(false)
            .exact_size(168.0)
            .show_inside(ui, |ui| {
                ui.add_space(12.0);
                nav_button(ui, &mut self.view, View::Today, "Today");
                nav_button(ui, &mut self.view, View::Inbox, "Inbox");
                nav_button(ui, &mut self.view, View::Projects, "Projects");
                nav_button(ui, &mut self.view, View::Schedule, "Schedule");
                nav_button(ui, &mut self.view, View::Assistant, "Assistant");
                nav_button(ui, &mut self.view, View::Activity, "Activity");
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
            View::Projects => self.show_projects(ui, palette),
            View::Schedule => self.show_schedule(ui, palette),
            View::Assistant => self.show_assistant(ui, palette),
            View::Activity => self.show_activity(ui, palette),
            View::Settings => self.show_settings(ui, palette),
        });
    }
}

impl MnemaGuiApp {
    fn show_today(&mut self, ui: &mut egui::Ui, palette: Palette) {
        section_header(ui, "Today", palette);
        ui.horizontal(|ui| {
            ui.label("Date");
            date_editor(ui, &mut self.target_date);
            if ui.button("Plan").clicked() {
                self.plan_today(false);
            }
            if ui.button("Save").clicked() {
                self.request_save_plan();
            }
        });
        ui.horizontal(|ui| {
            ui.label("Repair from");
            time_editor(ui, &mut self.repair_from_time);
            if ui.button("Repair").clicked() {
                self.repair_schedule(false);
            }
            if ui.button("Save repair").clicked() {
                self.request_save_repair();
            }
        });
        if self.confirm_plan_save {
            ui.horizontal(|ui| {
                ui.colored_label(
                    palette.warning,
                    "Existing proposed blocks for this date will be replaced.",
                );
                if ui.button("Replace proposed").clicked() {
                    self.confirm_plan_save = false;
                    self.plan_today(true);
                }
                if ui.button("Cancel").clicked() {
                    self.confirm_plan_save = false;
                    self.message = String::from("Save cancelled");
                }
            });
        }
        if self.confirm_repair_save {
            ui.horizontal(|ui| {
                ui.colored_label(
                    palette.warning,
                    "Repair will replace existing proposed blocks for this date.",
                );
                if ui.button("Replace with repair").clicked() {
                    self.confirm_repair_save = false;
                    self.repair_schedule(true);
                }
                if ui.button("Cancel").clicked() {
                    self.confirm_repair_save = false;
                    self.message = String::from("Repair save cancelled");
                }
            });
        }
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
        ui.label(bold_text("Tasks"));
        if let Some(action) = task_list(ui, &self.tasks, palette) {
            self.handle_task_action(action);
        }
        self.show_task_editor(ui, palette);
    }

    fn show_inbox(&mut self, ui: &mut egui::Ui, palette: Palette) {
        section_header(ui, "Inbox", palette);
        ui.horizontal(|ui| {
            ui.add_sized(
                [560.0, INPUT_HEIGHT],
                text_field(
                    &mut self.quick_capture,
                    "Quick capture: Write proposal tomorrow 45m",
                ),
            );
            if ui.button("Capture").clicked() {
                self.capture_quick_task();
            }
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add_sized(
                [340.0, INPUT_HEIGHT],
                text_field(&mut self.task_title, "Task title"),
            );
            ui.label("Due");
            date_editor(ui, &mut self.due_date);
            ui.label("Estimate");
            minutes_editor(ui, &mut self.minutes);
            if ui.button("Add").clicked() {
                self.add_task();
            }
        });
        ui.add_space(12.0);
        if let Some(action) = task_list(ui, &self.tasks, palette) {
            self.handle_task_action(action);
        }
        self.show_task_editor(ui, palette);
    }

    fn show_assistant(&mut self, ui: &mut egui::Ui, palette: Palette) {
        section_header(ui, "Assistant", palette);
        ui.label(regular_text(assistant_provider_label(self.llm_provider)).color(palette.muted));
        ui.add_space(8.0);
        ScrollArea::vertical().max_height(480.0).show(ui, |ui| {
            for message in &self.assistant_messages {
                ui.group(|ui| {
                    ui.label(bold_text(message.role).color(palette.section));
                    ui.label(message.content.as_str());
                });
                ui.add_space(8.0);
            }
        });
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.add_sized(
                [560.0, INPUT_HEIGHT],
                text_field(&mut self.assistant_input, "Ask or capture a task"),
            );
            if ui.button("Send").clicked() {
                self.send_assistant_message();
            }
        });
    }

    fn show_projects(&mut self, ui: &mut egui::Ui, palette: Palette) {
        section_header(ui, "Projects", palette);
        ui.horizontal(|ui| {
            ui.add_sized(
                [260.0, INPUT_HEIGHT],
                text_field(&mut self.project_title, "Project title"),
            );
            ui.label("Start");
            date_editor(ui, &mut self.project_start_date);
            ui.label("End");
            date_editor(ui, &mut self.project_end_date);
            if ui.button("Add").clicked() {
                self.add_project();
            }
        });
        ui.add_space(12.0);

        let mut select_project = None;
        ui.columns(2, |columns| {
            columns[0].label(bold_text("Projects").color(palette.section));
            columns[0].add_space(6.0);
            ScrollArea::vertical()
                .max_height(520.0)
                .show(&mut columns[0], |ui| {
                    for project in &self.projects {
                        let selected = self.selected_project_id.as_ref() == Some(&project.id);
                        ui.horizontal(|ui| {
                            if ui
                                .add_sized(
                                    [120.0, 30.0],
                                    egui::Button::selectable(selected, "Select"),
                                )
                                .clicked()
                            {
                                select_project = Some(project.id.clone());
                            }
                            ui.label(bold_text(project.title.as_str()));
                        });
                        let mut fields = Vec::new();
                        if let Some(start_date) = project.start_date {
                            fields.push(format!("start {start_date}"));
                        }
                        if let Some(end_date) = project.end_date {
                            fields.push(format!("end {end_date}"));
                        }
                        if !fields.is_empty() {
                            ui.label(regular_text(fields.join(", ")).color(palette.muted));
                        }
                        ui.separator();
                    }
                });

            columns[1].label(bold_text("Selected").color(palette.section));
            columns[1].add_space(6.0);
            if let Some(project_title) = self.selected_project_title() {
                columns[1].label(bold_text(project_title));
                columns[1].add_space(8.0);
                self.show_project_children(&mut columns[1], palette);
            } else {
                columns[1].label("No project selected.");
            }
        });

        if let Some(project_id) = select_project {
            self.selected_project_id = Some(project_id);
            self.refresh_project_children();
        }
    }

    fn show_project_children(&mut self, ui: &mut egui::Ui, palette: Palette) {
        ui.label(bold_text("Lists").color(palette.section));
        ui.horizontal(|ui| {
            ui.add_sized(
                [220.0, INPUT_HEIGHT],
                text_field(&mut self.project_list_name, "List name"),
            );
            if ui.button("Add list").clicked() {
                self.add_project_list();
            }
        });
        ui.add_space(6.0);
        if self.project_lists.is_empty() {
            ui.label("No lists.");
        } else {
            for list in &self.project_lists {
                ui.label(format!(
                    "{} · {}",
                    list.name,
                    list_view_type_label(&list.view_type)
                ));
            }
        }

        ui.add_space(14.0);
        ui.label(bold_text("Milestones").color(palette.section));
        ui.horizontal(|ui| {
            ui.add_sized(
                [220.0, INPUT_HEIGHT],
                text_field(&mut self.milestone_title, "Milestone title"),
            );
            ui.label("Target");
            date_editor(ui, &mut self.milestone_target_date);
            if ui.button("Add milestone").clicked() {
                self.add_milestone();
            }
        });
        ui.add_space(6.0);
        if self.milestones.is_empty() {
            ui.label("No milestones.");
        } else {
            for milestone in &self.milestones {
                ui.horizontal(|ui| {
                    ui.label(bold_text(milestone.title.as_str()));
                    ui.label(regular_text(milestone.target_date.to_string()).color(palette.due));
                    ui.label(
                        regular_text(milestone_status_label(&milestone.status))
                            .color(palette.muted),
                    );
                });
            }
        }
    }

    fn show_activity(&mut self, ui: &mut egui::Ui, palette: Palette) {
        section_header(ui, "Activity", palette);
        ui.horizontal(|ui| {
            if ui.button("Refresh").clicked() {
                self.refresh_automation_logs();
            }
            ui.label(format!("{} logs", self.automation_logs.len()));
        });
        ui.add_space(10.0);

        if self.automation_logs.is_empty() {
            ui.label("No activity yet.");
            return;
        }

        ScrollArea::vertical().show(ui, |ui| {
            for log in &self.automation_logs {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(bold_text(automation_action_label(&log.action_type)));
                        ui.label(regular_text(format_hm(log.created_at)).color(palette.muted));
                    });
                    if let Some(title) = automation_log_title(log) {
                        ui.label(title);
                    }
                    if let Some(explanation) = &log.explanation {
                        ui.label(regular_text(explanation.as_str()).color(palette.muted));
                    }
                });
                ui.add_space(8.0);
            }
        });
    }

    fn show_schedule(&mut self, ui: &mut egui::Ui, palette: Palette) {
        section_header(ui, "Schedule", palette);
        ui.horizontal(|ui| {
            ui.label("Date");
            date_editor(ui, &mut self.target_date);
            if ui.button("Load").clicked() {
                self.refresh_schedule();
            }
        });
        ui.add_space(12.0);

        let target_date = match parse_required_date(&self.target_date) {
            Ok(date) => date,
            Err(error) => {
                ui.colored_label(palette.error, error.to_string());
                return;
            }
        };

        if self.schedule.is_empty() {
            ui.label("No saved schedule blocks.");
            return;
        }

        if let Some(action) = schedule_calendar_view(ui, target_date, &self.schedule, palette) {
            self.handle_schedule_action(action);
        }
        self.show_schedule_block_editor(ui, palette);
    }

    fn show_settings(&mut self, ui: &mut egui::Ui, palette: Palette) {
        section_header(ui, "Settings", palette);
        egui::Grid::new("settings_grid")
            .num_columns(2)
            .spacing([14.0, 10.0])
            .show(ui, |ui| {
                ui.label("Vault");
                ui.add_sized(
                    [520.0, INPUT_HEIGHT],
                    text_field(&mut self.vault_path, "Vault path"),
                );
                ui.end_row();

                ui.label("Backend");
                ui.horizontal(|ui| {
                    ui.radio_value(
                        &mut self.selected_backend,
                        DesktopStorageBackend::Sqlite,
                        "SQLite",
                    );
                    ui.radio_value(
                        &mut self.selected_backend,
                        DesktopStorageBackend::Postgres,
                        "PostgreSQL",
                    );
                });
                ui.end_row();

                ui.label("SQLite DB");
                ui.horizontal(|ui| {
                    ui.add_sized(
                        [420.0, INPUT_HEIGHT],
                        text_field(&mut self.sqlite_path, "SQLite database path"),
                    );
                    if ui.button("Default").clicked() {
                        self.sqlite_path = default_sqlite_path().display().to_string();
                    }
                });
                ui.end_row();

                ui.label("PostgreSQL URL");
                ui.add_sized(
                    [520.0, INPUT_HEIGHT],
                    text_field(&mut self.database_url, DEFAULT_POSTGRES_URL),
                );
                ui.end_row();

                ui.label("LLM provider");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.llm_provider, DesktopLlmProvider::Disabled, "Off");
                    ui.radio_value(&mut self.llm_provider, DesktopLlmProvider::Ollama, "Ollama");
                    ui.radio_value(
                        &mut self.llm_provider,
                        DesktopLlmProvider::OpenAiCompatible,
                        "OpenAI compatible",
                    );
                });
                ui.end_row();

                ui.label("Ollama URL");
                ui.add_sized(
                    [520.0, INPUT_HEIGHT],
                    text_field(&mut self.ollama_url, "http://localhost:11434"),
                );
                ui.end_row();

                ui.label("OpenAI URL");
                ui.add_sized(
                    [520.0, INPUT_HEIGHT],
                    text_field(&mut self.openai_url, "https://api.openai.com"),
                );
                ui.end_row();

                ui.label("Planning model");
                ui.add_sized(
                    [260.0, INPUT_HEIGHT],
                    text_field(&mut self.planning_model, "gpt-4.1"),
                );
                ui.end_row();

                ui.label("Routine model");
                ui.add_sized(
                    [260.0, INPUT_HEIGHT],
                    text_field(&mut self.routine_model, "gpt-4.1-mini"),
                );
                ui.end_row();
            });

        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ui.button("Save settings").clicked() {
                self.save_settings();
            }
            if ui.button("Connect").clicked() {
                self.connect_and_refresh();
            }
        });
        ui.add_space(10.0);
        ui.label(format!(
            "Connected backend: {}",
            self.backend
                .map(|backend| match backend {
                    StorageBackend::Sqlite => "SQLite",
                    StorageBackend::Postgres => "PostgreSQL",
                })
                .unwrap_or("Disconnected")
        ));
        ui.label(format!(
            "Selected backend: {}",
            self.selected_backend.label()
        ));
        ui.label(format!("Config: {}", config_path().display()));
        if !self.settings_message.is_empty() {
            ui.label(regular_text(self.settings_message.as_str()).color(palette.success));
        }
    }

    fn show_task_editor(&mut self, ui: &mut egui::Ui, palette: Palette) {
        if self.editing_task_id.is_none() {
            return;
        }

        ui.add_space(8.0);
        ui.separator();
        ui.label(bold_text("Edit task").color(palette.section));
        let mut save = false;
        let mut cancel = false;
        ui.horizontal(|ui| {
            ui.add_sized(
                [340.0, INPUT_HEIGHT],
                text_field(&mut self.edit_title, "Task title"),
            );
            ui.label("Due");
            date_editor(ui, &mut self.edit_due_date);
            ui.label("Estimate");
            minutes_editor(ui, &mut self.edit_minutes);
            if ui.button("Save").clicked() {
                save = true;
            }
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
        });

        if save {
            self.save_task_edit();
        } else if cancel {
            self.clear_task_editor();
        }
    }

    fn show_schedule_block_editor(&mut self, ui: &mut egui::Ui, palette: Palette) {
        if self.editing_schedule_block_id.is_none() {
            return;
        }

        ui.add_space(8.0);
        ui.separator();
        ui.label(bold_text("Edit schedule time").color(palette.section));
        let mut save = false;
        let mut cancel = false;
        ui.horizontal(|ui| {
            ui.label("Start");
            time_editor(ui, &mut self.schedule_edit_start);
            ui.label("End");
            time_editor(ui, &mut self.schedule_edit_end);
            if ui.button("Save").clicked() {
                save = true;
            }
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
        });

        if save {
            self.save_schedule_block_edit();
        } else if cancel {
            self.clear_schedule_block_editor();
        }
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
    switch_bg: Color32,
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
            switch_bg: mix_color(
                Color32::from_rgb(224, 231, 239),
                Color32::from_rgb(18, 25, 36),
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

fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    if let Some(font_bytes) = load_noto_sans_jp() {
        let font_name = "noto_sans_jp".to_owned();
        fonts.font_data.insert(
            font_name.clone(),
            Arc::new(FontData::from_owned(font_bytes).tweak(FontTweak {
                coords: egui::epaint::text::VariationCoords::new([("wght", FONT_WEIGHT_REGULAR)]),
                ..Default::default()
            })),
        );

        if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
            family.insert(0, font_name.clone());
        }
        if let Some(family) = fonts.families.get_mut(&FontFamily::Monospace) {
            family.push(font_name);
        }
    }

    if let Some(font_bytes) = load_montserrat() {
        let font_name = "montserrat".to_owned();
        fonts.font_data.insert(
            font_name.clone(),
            Arc::new(FontData::from_owned(font_bytes).tweak(FontTweak {
                coords: egui::epaint::text::VariationCoords::new([("wght", FONT_WEIGHT_BOLD)]),
                ..Default::default()
            })),
        );
        fonts
            .families
            .insert(FontFamily::Name(LOGO_FONT_FAMILY.into()), vec![font_name]);
    }

    ctx.set_fonts(fonts);
}

fn load_noto_sans_jp() -> Option<Vec<u8>> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    [
        manifest_dir.join("assets/fonts/NotoSansJP-VF.ttf"),
        manifest_dir.join("assets/fonts/NotoSansJP-Regular.ttf"),
        PathBuf::from(r"C:\Windows\Fonts\NotoSansJP-VF.ttf"),
        PathBuf::from(r"C:\Windows\Fonts\NotoSansJP-Regular.ttf"),
    ]
    .into_iter()
    .find_map(|path| fs::read(path).ok())
}

fn load_montserrat() -> Option<Vec<u8>> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    [manifest_dir.join("assets/fonts/Montserrat-VF.ttf")]
        .into_iter()
        .find_map(|path| fs::read(path).ok())
}

fn configure_style(ctx: &egui::Context, dark_factor: f32, dark_mode: bool) {
    let palette = Palette::at(dark_factor);
    let mut style = egui::Theme::from_dark_mode(dark_mode).default_style();
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 7.0);
    style.visuals = themed_visuals(palette, dark_mode);
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
    ui.allocate_ui_with_layout(
        THEME_SWITCH_SIZE,
        egui::Layout::left_to_right(Align::Center),
        |ui| {
            let rect = ui.max_rect();
            ui.painter().rect(
                rect,
                16.0,
                palette.switch_bg,
                Stroke::new(1.0, palette.border),
                egui::StrokeKind::Inside,
            );
            ui.spacing_mut().button_padding = egui::vec2(5.0, 4.0);
            ui.spacing_mut().item_spacing.x = 3.0;
            ui.add_space(3.0);
            if theme_icon_button(ui, !*dark_mode, "☀", "Light", palette).clicked() {
                *dark_mode = false;
            }
            if theme_icon_button(ui, *dark_mode, "🌙", "Dark", palette).clicked() {
                *dark_mode = true;
            }
        },
    );
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
    let mut button = egui::Button::selectable(selected, text).corner_radius(16.0);
    if selected {
        button = button.fill(palette.selected_fill);
    }
    ui.add_sized([30.0, 30.0], button).on_hover_text(hover_text)
}

fn regular_text(text: impl Into<String>) -> RichText {
    RichText::new(text).variation("wght", FONT_WEIGHT_REGULAR)
}

fn bold_text(text: impl Into<String>) -> RichText {
    regular_text(text)
        .variation("wght", FONT_WEIGHT_BOLD)
        .strong()
}

fn logo_text(text: impl Into<String>) -> RichText {
    RichText::new(text)
        .family(FontFamily::Name(LOGO_FONT_FAMILY.into()))
        .variation("wght", FONT_WEIGHT_BOLD)
        .strong()
}

fn text_field<'a>(value: &'a mut String, hint_text: &'static str) -> TextEdit<'a> {
    TextEdit::singleline(value)
        .hint_text(hint_text)
        .vertical_align(Align::Center)
}

fn date_editor(ui: &mut egui::Ui, value: &mut String) {
    ui.add_sized([118.0, INPUT_HEIGHT], text_field(value, "YYYY-MM-DD"));
    if ui.button("‹").clicked() {
        shift_date(value, -1);
    }
    if ui.button("Today").clicked() {
        *value = OffsetDateTime::now_utc().date().to_string();
    }
    if ui.button("›").clicked() {
        shift_date(value, 1);
    }
}

fn shift_date(value: &mut String, days: i64) {
    let base = parse_optional_date(value)
        .ok()
        .flatten()
        .unwrap_or_else(|| OffsetDateTime::now_utc().date());
    let shifted = if days < 0 {
        base.previous_day().unwrap_or(base)
    } else {
        base.next_day().unwrap_or(base)
    };
    *value = shifted.to_string();
}

fn minutes_editor(ui: &mut egui::Ui, value: &mut String) {
    let mut minutes = value.trim().parse::<u32>().unwrap_or(0);
    let response = ui.add_sized(
        [92.0, INPUT_HEIGHT],
        egui::DragValue::new(&mut minutes)
            .range(0..=24 * 60)
            .speed(5)
            .suffix(" min"),
    );
    if response.changed() {
        *value = minutes.to_string();
    }
    if ui.button("Clear").clicked() {
        value.clear();
    }
}

fn time_editor(ui: &mut egui::Ui, value: &mut String) {
    ui.add_sized([74.0, INPUT_HEIGHT], text_field(value, "HH:MM"));
    if ui.button("-15").clicked() {
        shift_time(value, -15);
    }
    if ui.button("+15").clicked() {
        shift_time(value, 15);
    }
}

fn shift_time(value: &mut String, minutes: i64) {
    let Ok(time) = Time::parse(value.trim(), format_description!("[hour]:[minute]")) else {
        return;
    };
    let date = OffsetDateTime::now_utc().date();
    let shifted = date
        .with_time(time)
        .assume_utc()
        .saturating_add(time::Duration::minutes(minutes));
    *value = format_hm(shifted);
}

fn themed_visuals(palette: Palette, dark_mode: bool) -> egui::Visuals {
    let mut visuals = egui::Theme::from_dark_mode(dark_mode).default_visuals();
    visuals.dark_mode = dark_mode;
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

fn localized_error_message(error: &anyhow::Error) -> String {
    let message = error.to_string();
    match message.as_str() {
        "task title is required" => "タスク名を入力してください".to_string(),
        "task not found" => "タスクが見つかりません".to_string(),
        "schedule block not found" => "予定ブロックが見つかりません".to_string(),
        "schedule block end must be after start" => {
            "終了時刻は開始時刻より後にしてください".to_string()
        }
        "vault is not connected" => "Vault に接続されていません".to_string(),
        _ => message,
    }
}

fn non_empty_or(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

fn assistant_provider_label(provider: DesktopLlmProvider) -> &'static str {
    match provider {
        DesktopLlmProvider::Disabled => "LLM: Off",
        DesktopLlmProvider::Ollama => "LLM: Ollama",
        DesktopLlmProvider::OpenAiCompatible => "LLM: OpenAI compatible",
    }
}

fn save_config(config: &DesktopConfig) -> Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(config)?;
    fs::write(path, bytes)?;
    Ok(())
}

fn config_path() -> PathBuf {
    if let Ok(path) = std::env::var("LOCALAPPDATA") {
        return PathBuf::from(path).join("Mnema").join("config.json");
    }
    if let Ok(path) = std::env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(path).join("mnema").join("config.json");
    }
    if let Ok(path) = std::env::var("HOME") {
        return PathBuf::from(path)
            .join(".config")
            .join("mnema")
            .join("config.json");
    }
    PathBuf::from("./mnema-config.json")
}

fn default_sqlite_path() -> PathBuf {
    if let Ok(path) = std::env::var("LOCALAPPDATA") {
        return PathBuf::from(path).join("Mnema").join("mnema.sqlite");
    }
    if let Ok(path) = std::env::var("XDG_DATA_HOME") {
        return PathBuf::from(path).join("mnema").join("mnema.sqlite");
    }
    if let Ok(path) = std::env::var("HOME") {
        return PathBuf::from(path)
            .join(".local")
            .join("share")
            .join("mnema")
            .join("mnema.sqlite");
    }
    PathBuf::from("./mnema.sqlite")
}

fn section_header(ui: &mut egui::Ui, title: &str, palette: Palette) {
    ui.add_space(12.0);
    ui.heading(bold_text(title).color(palette.section));
    ui.add_space(8.0);
}

fn task_list(ui: &mut egui::Ui, tasks: &[Task], palette: Palette) -> Option<TaskAction> {
    if tasks.is_empty() {
        ui.label("No tasks.");
        return None;
    }

    let mut action = None;
    ScrollArea::vertical().show(ui, |ui| {
        for task in tasks {
            ui.horizontal(|ui| {
                ui.label(bold_text(task.title.as_str()));
                if let Some(due_date) = task.due_date {
                    ui.label(regular_text(format!("due {due_date}")).color(palette.due));
                }
                if let Some(minutes) = task.estimated_minutes {
                    ui.label(format!("{minutes}m"));
                }
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Delete").clicked() {
                        action = Some(TaskAction::Delete(task.id.clone()));
                    }
                    if ui.button("Done").clicked() {
                        action = Some(TaskAction::Complete(task.id.clone()));
                    }
                    if ui.button("Edit").clicked() {
                        action = Some(TaskAction::Edit(task.id.clone()));
                    }
                });
            });
            ui.separator();
        }
    });
    action
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
                ui.label(bold_text(block.title.as_str()));
                ui.label(format!("{}m", block.required_minutes));
            });
            ui.separator();
        }
    });
}

fn schedule_calendar_view(
    ui: &mut egui::Ui,
    target_date: Date,
    blocks: &[ScheduleBlock],
    palette: Palette,
) -> Option<ScheduleAction> {
    let mut action = None;
    ui.horizontal_top(|ui| {
        schedule_timeline(ui, target_date, blocks, palette);
        ui.add_space(12.0);
        action = schedule_action_panel(ui, blocks, palette);
    });
    action
}

fn schedule_timeline(
    ui: &mut egui::Ui,
    target_date: Date,
    blocks: &[ScheduleBlock],
    palette: Palette,
) {
    let Ok((day_start, day_end)) = schedule_bounds(target_date, blocks) else {
        ui.colored_label(palette.error, "Invalid schedule bounds.");
        return;
    };
    let total_minutes = (day_end - day_start).whole_minutes().max(60) as f32;
    let height = (total_minutes * 1.15).clamp(420.0, 960.0);
    let width = (ui.available_width() - 260.0).clamp(420.0, 760.0);

    ScrollArea::vertical().max_height(560.0).show(ui, |ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect(
            rect,
            8.0,
            palette.surface,
            Stroke::new(1.0, palette.border),
            egui::StrokeKind::Inside,
        );

        let label_width = 58.0;
        let content_left = rect.left() + label_width;
        let content_right = rect.right() - 10.0;
        let pixels_per_minute = rect.height() / total_minutes;
        let mut hour = day_start.hour();
        let end_hour = day_end.hour();

        while hour <= end_hour {
            let Ok(mark) = target_date.with_hms(hour, 0, 0) else {
                break;
            };
            let mark = mark.assume_utc();
            let y = rect.top() + ((mark - day_start).whole_minutes() as f32 * pixels_per_minute);
            if rect.contains(egui::pos2(content_left, y)) {
                painter.text(
                    egui::pos2(rect.left() + 10.0, y),
                    egui::Align2::LEFT_CENTER,
                    format!("{hour:02}:00"),
                    egui::FontId::proportional(12.0),
                    palette.muted,
                );
                painter.line_segment(
                    [egui::pos2(content_left, y), egui::pos2(content_right, y)],
                    Stroke::new(1.0, palette.faint),
                );
            }
            hour += 1;
        }

        for block in blocks {
            draw_schedule_block(&painter, rect, day_start, total_minutes, block, palette);
        }
    });
}

fn draw_schedule_block(
    painter: &egui::Painter,
    rect: egui::Rect,
    day_start: OffsetDateTime,
    total_minutes: f32,
    block: &ScheduleBlock,
    palette: Palette,
) {
    let label_width = 58.0;
    let left = rect.left() + label_width + 10.0;
    let right = rect.right() - 12.0;
    let pixels_per_minute = rect.height() / total_minutes;
    let start_minutes = (block.start_at - day_start).whole_minutes() as f32;
    let end_minutes = (block.end_at - day_start).whole_minutes() as f32;
    let top = rect
        .top()
        .max(rect.top() + start_minutes * pixels_per_minute + 2.0);
    let bottom = rect
        .bottom()
        .min(rect.top() + end_minutes * pixels_per_minute - 2.0);
    if bottom <= top {
        return;
    }

    let block_rect = egui::Rect::from_min_max(
        egui::pos2(left, top),
        egui::pos2(right, bottom.max(top + 30.0)),
    );
    let (fill, stroke, text_color) = schedule_block_colors(&block.state, palette);
    painter.rect(
        block_rect,
        6.0,
        fill,
        Stroke::new(1.0, stroke),
        egui::StrokeKind::Inside,
    );

    let title = block
        .title_snapshot
        .as_deref()
        .unwrap_or("(untitled block)");
    let duration = schedule_block_minutes(block);
    let label = format!(
        "{}-{}  {}  {duration}m",
        format_hm(block.start_at),
        format_hm(block.end_at),
        title
    );
    painter.text(
        block_rect.left_top() + egui::vec2(10.0, 8.0),
        egui::Align2::LEFT_TOP,
        label,
        egui::FontId::proportional(13.0),
        text_color,
    );
}

fn schedule_action_panel(
    ui: &mut egui::Ui,
    blocks: &[ScheduleBlock],
    palette: Palette,
) -> Option<ScheduleAction> {
    let mut action = None;
    ui.allocate_ui_with_layout(
        egui::vec2(260.0, 560.0),
        egui::Layout::top_down(Align::Min),
        |ui| {
            ui.label(bold_text("Blocks").color(palette.section));
            ui.add_space(6.0);
            ScrollArea::vertical().show(ui, |ui| {
                for block in blocks {
                    ui.group(|ui| {
                        ui.label(bold_text(
                            block
                                .title_snapshot
                                .as_deref()
                                .unwrap_or("(untitled block)"),
                        ));
                        ui.horizontal(|ui| {
                            ui.monospace(format!(
                                "{}-{}",
                                format_hm(block.start_at),
                                format_hm(block.end_at)
                            ));
                            ui.label(format!("{}m", schedule_block_minutes(block)));
                        });
                        ui.label(
                            regular_text(schedule_state_label(&block.state))
                                .color(schedule_state_text_color(&block.state, palette)),
                        );
                        ui.horizontal(|ui| {
                            if ui.button("Edit").clicked() {
                                action = Some(ScheduleAction::Edit(block.id.clone()));
                            }
                            if block.state == ScheduleBlockState::Proposed
                                && ui.button("Schedule").clicked()
                            {
                                action = Some(ScheduleAction::SetState(
                                    block.id.clone(),
                                    ScheduleBlockState::Scheduled,
                                ));
                            }
                            if block.state != ScheduleBlockState::Done
                                && ui.button("Done").clicked()
                            {
                                action = Some(ScheduleAction::SetState(
                                    block.id.clone(),
                                    ScheduleBlockState::Done,
                                ));
                            }
                            if block.state != ScheduleBlockState::Cancelled
                                && ui.button("Cancel").clicked()
                            {
                                action = Some(ScheduleAction::SetState(
                                    block.id.clone(),
                                    ScheduleBlockState::Cancelled,
                                ));
                            }
                        });
                    });
                    ui.add_space(8.0);
                }
            });
        },
    );
    action
}

fn schedule_bounds(
    target_date: Date,
    blocks: &[ScheduleBlock],
) -> Result<(OffsetDateTime, OffsetDateTime)> {
    let mut start = target_date.with_hms(9, 0, 0)?.assume_utc();
    let mut end = target_date.with_hms(17, 0, 0)?.assume_utc();
    for block in blocks {
        start = start.min(block.start_at);
        end = end.max(block.end_at);
    }
    Ok((start, end))
}

fn schedule_block_colors(
    state: &ScheduleBlockState,
    palette: Palette,
) -> (Color32, Color32, Color32) {
    match state {
        ScheduleBlockState::Proposed => (palette.selected_fill, palette.accent, palette.text),
        ScheduleBlockState::Scheduled | ScheduleBlockState::Active => {
            (palette.control_bg, palette.accent, palette.text)
        }
        ScheduleBlockState::Done => (palette.faint, palette.success, palette.success),
        ScheduleBlockState::Missed => (palette.faint, palette.warning, palette.warning),
        ScheduleBlockState::Cancelled => (palette.faint, palette.border, palette.muted),
    }
}

fn schedule_state_text_color(state: &ScheduleBlockState, palette: Palette) -> Color32 {
    match state {
        ScheduleBlockState::Done => palette.success,
        ScheduleBlockState::Missed => palette.warning,
        ScheduleBlockState::Cancelled => palette.muted,
        _ => palette.accent,
    }
}

fn schedule_block_minutes(block: &ScheduleBlock) -> i64 {
    (block.end_at - block.start_at).whole_minutes().max(0)
}

async fn done_status_ids(
    statuses: &dyn StatusRepository,
    tasks: &[Task],
) -> Result<HashSet<StatusId>> {
    let done_group_ids = statuses
        .list_groups()
        .await?
        .into_iter()
        .filter(|group| group.kind == StatusGroupKind::Done)
        .map(|group| group.id)
        .collect::<HashSet<_>>();
    let mut project_ids = tasks
        .iter()
        .filter_map(|task| task.project_id.clone())
        .collect::<HashSet<_>>();
    let mut status_ids = HashSet::new();

    for status in statuses.list_statuses_for_project(None).await? {
        if done_group_ids.contains(&status.group_id) {
            status_ids.insert(status.id);
        }
    }

    for project_id in project_ids.drain() {
        for status in statuses.list_statuses_for_project(Some(project_id)).await? {
            if done_group_ids.contains(&status.group_id) {
                status_ids.insert(status.id);
            }
        }
    }

    Ok(status_ids)
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
    Date::parse(value.trim(), format_description!("[year]-[month]-[day]"))
        .map_err(|_| anyhow!("日付は YYYY-MM-DD で入力してください"))
}

fn parse_hm_for_date(value: &str, date: Date) -> Result<OffsetDateTime> {
    let time = Time::parse(value.trim(), format_description!("[hour]:[minute]"))
        .map_err(|_| anyhow!("時刻は HH:MM で入力してください"))?;
    Ok(date.with_time(time).assume_utc())
}

fn parse_optional_u32(value: &str) -> Result<Option<u32>> {
    let value = value.trim();
    if value.is_empty() {
        Ok(None)
    } else {
        value
            .parse::<u32>()
            .map(Some)
            .map_err(|_| anyhow!("見積分数は数値で入力してください"))
    }
}

fn parse_quick_capture(value: &str, today: Date) -> Result<CaptureTaskRequest> {
    let value = value.trim();
    if value.is_empty() {
        return Err(anyhow!("タスク名を入力してください"));
    }

    let tokens = value.split_whitespace().collect::<Vec<_>>();
    let mut title_tokens = Vec::new();
    let mut due_date = None;
    let mut estimated_minutes = None;
    let mut index = 0;

    while index < tokens.len() {
        let token = tokens[index];
        let normalized = token
            .trim_matches(|ch: char| ch == ',' || ch == ';')
            .to_ascii_lowercase();

        if matches!(normalized.as_str(), "today" | "今日") {
            due_date = Some(today);
            index += 1;
            continue;
        }

        if matches!(normalized.as_str(), "tomorrow" | "明日") {
            due_date = Some(today.next_day().unwrap_or(today));
            index += 1;
            continue;
        }

        if matches!(normalized.as_str(), "/due" | "due") {
            let Some(raw_due) = tokens.get(index + 1) else {
                return Err(anyhow!("期限日を入力してください"));
            };
            due_date = Some(parse_required_date(raw_due)?);
            index += 2;
            continue;
        }

        if let Some(raw_due) = normalized
            .strip_prefix("/due:")
            .or_else(|| normalized.strip_prefix("due:"))
        {
            due_date = Some(parse_required_date(raw_due)?);
            index += 1;
            continue;
        }

        if matches!(normalized.as_str(), "/m" | "/minutes" | "minutes") {
            let Some(raw_minutes) = tokens.get(index + 1) else {
                return Err(anyhow!("見積分数を入力してください"));
            };
            estimated_minutes = Some(
                raw_minutes
                    .parse::<u32>()
                    .map_err(|_| anyhow!("見積分数は数値で入力してください"))?,
            );
            index += 2;
            continue;
        }

        if let Some(minutes) = parse_duration_token(&normalized) {
            estimated_minutes = Some(minutes);
            index += 1;
            continue;
        }

        title_tokens.push(token);
        index += 1;
    }

    let title = title_tokens.join(" ").trim().to_string();
    if title.is_empty() {
        return Err(anyhow!("タスク名を入力してください"));
    }

    Ok(CaptureTaskRequest {
        title,
        description: None,
        due_date,
        estimated_minutes,
    })
}

fn parse_duration_token(token: &str) -> Option<u32> {
    let minutes = token
        .strip_suffix("minutes")
        .or_else(|| token.strip_suffix("minute"))
        .or_else(|| token.strip_suffix("mins"))
        .or_else(|| token.strip_suffix("min"))
        .or_else(|| token.strip_suffix('m'))
        .and_then(|value| value.parse::<u32>().ok());
    if minutes.is_some() {
        return minutes;
    }

    token
        .strip_suffix("hours")
        .or_else(|| token.strip_suffix("hour"))
        .or_else(|| token.strip_suffix("hrs"))
        .or_else(|| token.strip_suffix("hr"))
        .or_else(|| token.strip_suffix('h'))
        .and_then(|value| value.parse::<f32>().ok())
        .map(|hours| (hours * 60.0).round().max(1.0) as u32)
}

fn asks_next_action(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    value.contains("what should i do next")
        || value.contains("next action")
        || value.contains("次")
        || value.contains("なにする")
        || value.contains("何する")
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

fn list_view_type_label(view_type: &ListViewType) -> &'static str {
    match view_type {
        ListViewType::List => "list",
        ListViewType::Board => "board",
        ListViewType::Calendar => "calendar",
        ListViewType::Gantt => "gantt",
    }
}

fn milestone_status_label(status: &MilestoneStatus) -> &'static str {
    match status {
        MilestoneStatus::NotDone => "not done",
        MilestoneStatus::Overdue => "overdue",
        MilestoneStatus::Done => "done",
    }
}

fn automation_action_label(action_type: &AutomationActionType) -> String {
    match action_type {
        AutomationActionType::Move => "Move".to_string(),
        AutomationActionType::UpdateDue => "Update due".to_string(),
        AutomationActionType::Classify => "Classify".to_string(),
        AutomationActionType::CreateTask => "Create task".to_string(),
        AutomationActionType::UpdateStatus => "Update status".to_string(),
        AutomationActionType::Other(value) => value.clone(),
    }
}

fn automation_log_title(log: &AutomationLog) -> Option<String> {
    log.after_state
        .as_ref()
        .and_then(|value| value.get("title"))
        .and_then(|value| value.as_str())
        .map(|title| format!("Task: {title}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn parses_quick_capture_due_and_minutes() {
        let request =
            parse_quick_capture("Write proposal tomorrow 45m", date!(2026 - 06 - 25)).unwrap();

        assert_eq!(request.title, "Write proposal");
        assert_eq!(request.due_date, Some(date!(2026 - 06 - 26)));
        assert_eq!(request.estimated_minutes, Some(45));
    }

    #[test]
    fn parses_quick_capture_slash_due_and_hours() {
        let request =
            parse_quick_capture("Review notes /due 2026-06-30 1.5h", date!(2026 - 06 - 25))
                .unwrap();

        assert_eq!(request.title, "Review notes");
        assert_eq!(request.due_date, Some(date!(2026 - 06 - 30)));
        assert_eq!(request.estimated_minutes, Some(90));
    }
}
