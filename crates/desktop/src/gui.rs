#![allow(clippy::too_many_arguments)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
};

use anyhow::{Result, anyhow};
use eframe::egui::{
    self, Align, Color32, FontData, FontDefinitions, FontFamily, FontTweak, RichText, ScrollArea,
    Stroke, TextEdit,
};
use mnema_app::{
    AutoSchedulePreview, AutoScheduleRequest, AutoScheduleService, CaptureTaskRequest,
    CaptureTaskService, HabitService, PlanTodayResult, ScheduleBlockCommandService, ScheduleIssue,
    SchedulePlanStoreService, TaskCommandService, UpdateScheduleBlockWindowRequest,
};
use mnema_core::prelude::*;
use mnema_infra::{
    db::{StorageBackend, Vault},
    llm::{ChatMessage, ChatRole, LlmClient, LlmConfig, OllamaClient, OpenAiCompatibleClient},
};
use muda::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use serde::{Deserialize, Serialize};
use time::{Date, Month, OffsetDateTime, Time, UtcOffset, Weekday, macros::format_description};
use time_tz::{OffsetDateTimeExt, timezones};
use tokio::runtime::Runtime;

use crate::scheduling_ui::{
    HabitDraft, HabitScheduleChoice, SchedulingForm, date_range_end, item_issue_label,
    normalize_selected_calendar_ids,
};

mod calendar;
mod components;
mod date_picker;
mod habits;
mod home;
mod task_fields;
mod timezone;
use components::theme_toggle;
use timezone::TimezoneMode;
#[cfg(test)]
mod interaction_tests;
mod pages;
mod task_capture;
mod tasks;

static MENU_EVENTS: OnceLock<Mutex<Vec<MenuEvent>>> = OnceLock::new();

pub fn run_gui(initial_vault_path: PathBuf) -> Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1360.0, 920.0])
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
    Home,
    Inbox,
    Projects,
    Schedule,
    Habits,
    Assistant,
    Activity,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScheduleViewMode {
    Day,
    Days(u8),
    Week,
    Calendar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProjectViewMode {
    Details,
    Gantt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
enum TaskListDensity {
    #[default]
    Normal,
    Compact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
enum TaskSortMode {
    #[default]
    DueDate,
    Estimate,
    Importance,
    Created,
}

impl TaskSortMode {
    fn label(self) -> &'static str {
        match self {
            Self::DueDate => "期限順",
            Self::Estimate => "見積もり時間順",
            Self::Importance => "重要度順",
            Self::Created => "作成順",
        }
    }
}

#[derive(Debug, Clone)]
enum TaskAction {
    SetStatus(TaskId, StatusId),
    RequestDelete(TaskId),
    ConfirmDelete(TaskId),
}

#[derive(Debug, Clone)]
struct TaskDragPayload {
    task_id: TaskId,
    title: String,
    estimated_minutes: Option<u32>,
}

#[derive(Debug, Clone)]
struct ScheduleBlockDragPayload {
    block_id: ScheduleBlockId,
    title: String,
    duration_minutes: i64,
    grab_offset_minutes: i64,
}

#[derive(Debug, Clone)]
struct ManualScheduleRequest {
    task_id: TaskId,
    title: String,
    start_at: OffsetDateTime,
    duration_minutes: u32,
}

#[derive(Debug, Clone)]
struct MoveScheduleBlockRequest {
    block_id: ScheduleBlockId,
    title: String,
    start_at: OffsetDateTime,
    duration_minutes: i64,
}

#[derive(Debug, Default)]
struct HomeAgendaOutput {
    repair_clicked: bool,
    manual_schedule: Option<ManualScheduleRequest>,
    move_schedule: Option<MoveScheduleBlockRequest>,
}

#[derive(Debug, Clone)]
struct StatusHistoryEntry {
    task_id: TaskId,
    task_title: String,
    before_status_id: StatusId,
    after_status_id: StatusId,
}

#[derive(Debug, Clone, Copy)]
enum MenuAction {
    Close,
    Undo,
    Redo,
    SetView(View),
    SetDarkMode(bool),
}

#[derive(Debug, Clone)]
struct AssistantChatMessage {
    role: &'static str,
    content: String,
}

const INPUT_HEIGHT: f32 = 34.0;
const FONT_WEIGHT_REGULAR: f32 = 400.0;
const FONT_WEIGHT_BOLD: f32 = 700.0;
const TEXT_REGULAR_FONT_FAMILY: &str = "mnema_text_regular";
const TEXT_BOLD_FONT_FAMILY: &str = "mnema_text_bold";
const LOGO_FONT_FAMILY: &str = "mnema_logo";
const MATERIAL_ICON_FONT_FAMILY: &str = "mnema_material_icons";
const DEFAULT_POSTGRES_URL: &str = "postgres://postgres:postgres@localhost/mnema";
const DEFAULT_TIMEZONE_OFFSET: &str = "Asia/Tokyo";

const ICON_ASSISTANT: char = '\u{e39f}';
const ICON_CHECK: char = '\u{e5ca}';
const ICON_DARK_MODE: char = '\u{e51c}';
const ICON_EVENT: char = '\u{e878}';
const ICON_FOLDER: char = '\u{e2c7}';
const ICON_HISTORY: char = '\u{e889}';
const ICON_HOME: char = '\u{e88a}';
const ICON_HABIT: char = '\u{e87d}';
const ICON_INBOX: char = '\u{e156}';
const ICON_LIGHT_MODE: char = '\u{e518}';
const ICON_SETTINGS: char = '\u{e8b8}';

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
    #[serde(default = "default_timezone_offset")]
    timezone_offset: String,
    #[serde(default)]
    timezone_mode: TimezoneMode,
    #[serde(default)]
    manual_timezone: String,
    #[serde(default = "default_availability_start")]
    availability_start: String,
    #[serde(default = "default_availability_end")]
    availability_end: String,
    #[serde(default)]
    home_task_density: TaskListDensity,
    #[serde(default)]
    home_task_sort: TaskSortMode,
    #[serde(default)]
    task_grouping: tasks::TaskGrouping,
    llm_provider: DesktopLlmProvider,
    ollama_url: String,
    openai_url: String,
    planning_model: String,
    routine_model: String,
    #[serde(default)]
    read_notifications: Vec<AutomationLogId>,
}

impl DesktopConfig {
    fn load(initial_vault_path: PathBuf, default_dark_mode: bool) -> Self {
        let fallback = Self {
            vault_path: initial_vault_path.display().to_string(),
            storage_backend: DesktopStorageBackend::Sqlite,
            sqlite_path: default_sqlite_path().display().to_string(),
            database_url: DEFAULT_POSTGRES_URL.to_string(),
            dark_mode: default_dark_mode,
            timezone_offset: default_timezone_offset(),
            timezone_mode: TimezoneMode::Automatic,
            manual_timezone: default_timezone_offset(),
            availability_start: default_availability_start(),
            availability_end: default_availability_end(),
            home_task_density: TaskListDensity::Normal,
            home_task_sort: TaskSortMode::DueDate,
            task_grouping: tasks::TaskGrouping::Schedule,
            llm_provider: DesktopLlmProvider::Disabled,
            ollama_url: "http://localhost:11434".to_string(),
            openai_url: "https://api.openai.com".to_string(),
            planning_model: "gpt-4.1".to_string(),
            routine_model: "gpt-4.1-mini".to_string(),
            read_notifications: Vec::new(),
        };

        let Ok(bytes) = fs::read(config_path()) else {
            return fallback;
        };

        serde_json::from_slice::<Self>(&bytes).unwrap_or(fallback)
    }
}

fn default_timezone_offset() -> String {
    DEFAULT_TIMEZONE_OFFSET.to_string()
}

fn default_availability_start() -> String {
    "09:00".to_string()
}

fn default_availability_end() -> String {
    "17:00".to_string()
}

fn default_timezone() -> UtcOffset {
    UtcOffset::from_hms(9, 0, 0).expect("default JST offset is valid")
}

fn parse_timezone_offset(value: &str) -> Result<UtcOffset> {
    let value = value.trim();
    if value.is_empty()
        || value.eq_ignore_ascii_case("jst")
        || value.eq_ignore_ascii_case("asia/tokyo")
    {
        return Ok(default_timezone());
    }
    if value.eq_ignore_ascii_case("utc") || value.eq_ignore_ascii_case("z") {
        return Ok(UtcOffset::UTC);
    }
    if let Some(timezone) = timezones::get_by_name(value) {
        return Ok(OffsetDateTime::now_utc().to_timezone(timezone).offset());
    }

    let Some(sign_char) = value.chars().next() else {
        return Err(anyhow!("timezoneは +09:00 の形式で入力してください"));
    };
    let sign = match sign_char {
        '+' => 1_i8,
        '-' => -1_i8,
        _ => return Err(anyhow!("timezoneは +09:00 の形式で入力してください")),
    };
    let rest = &value[1..];
    let parts = rest.split(':').collect::<Vec<_>>();
    let [hours, minutes] = parts.as_slice() else {
        return Err(anyhow!("timezoneは +09:00 の形式で入力してください"));
    };
    let hours = hours
        .parse::<i8>()
        .map_err(|_| anyhow!("timezone hourが不正です"))?;
    let minutes = minutes
        .parse::<i8>()
        .map_err(|_| anyhow!("timezone minuteが不正です"))?;
    UtcOffset::from_hms(sign * hours, sign * minutes, 0)
        .map_err(|_| anyhow!("timezone offsetが不正です"))
}

struct NativeMenu {
    root: Menu,
    close: MenuItem,
    undo: PredefinedMenuItem,
    redo: PredefinedMenuItem,
    home: MenuItem,
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
        let close = MenuItem::with_id("mnema.file.close", "&Close", true, None);
        let file_menu = Submenu::with_items("&File", true, &[&close])?;

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

        let home = MenuItem::with_id("mnema.view.home", "&Home", true, None);
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
                &home,
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
            close,
            undo,
            redo,
            home,
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
        if id == self.close.id().as_ref() {
            Some(MenuAction::Close)
        } else if id == self.undo.id().as_ref() {
            Some(MenuAction::Undo)
        } else if id == self.redo.id().as_ref() {
            Some(MenuAction::Redo)
        } else if id == self.home.id().as_ref() {
            Some(MenuAction::SetView(View::Home))
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
    timezone_offset: String,
    timezone_mode: TimezoneMode,
    applied_timezone_mode: TimezoneMode,
    manual_timezone: String,
    detected_timezone: Option<String>,
    timezone_error: Option<String>,
    availability_start: String,
    availability_end: String,
    sleep_start: String,
    sleep_end: String,
    travel_buffer_minutes: String,
    view: View,
    last_view: View,
    quick_capture: String,
    assistant_input: String,
    assistant_messages: Vec<AssistantChatMessage>,
    editing_schedule_block_id: Option<ScheduleBlockId>,
    schedule_edit_start: String,
    schedule_edit_end: String,
    target_date: String,
    scroll_home_agenda_to_now: bool,
    home_filter: home::TaskFilter,
    home_calendar_mode: ScheduleViewMode,
    workspace_ui: pages::WorkspaceUi,
    tasks: Vec<Task>,
    done_tasks: Vec<Task>,
    lists: Vec<List>,
    statuses: Vec<Status>,
    status_groups: Vec<StatusGroup>,
    confirming_delete_task_id: Option<TaskId>,
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
    project_view_mode: ProjectViewMode,
    plan: Option<PlanTodayResult>,
    auto_preview: Option<AutoSchedulePreview>,
    planning_days: u32,
    schedule: Vec<ScheduleBlock>,
    schedule_month: Vec<ScheduleBlock>,
    automation_logs: Vec<AutomationLog>,
    scheduling_preferences: Option<SchedulingPreferences>,
    habits: Vec<Habit>,
    habit_occurrences: Vec<HabitOccurrence>,
    habit_draft: HabitDraft,
    calendar_accounts: Vec<CalendarAccount>,
    calendar_sync_cursors: Vec<CalendarSyncCursor>,
    calendar_selection_edits: HashMap<CalendarAccountId, String>,
    undo_stack: Vec<StatusHistoryEntry>,
    redo_stack: Vec<StatusHistoryEntry>,
    message: String,
    error: Option<String>,
    dark_mode: bool,
    home_task_density: TaskListDensity,
    home_task_sort: TaskSortMode,
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
        Self::from_config(config, native_menu)
    }

    fn from_config(mut config: DesktopConfig, native_menu: Option<NativeMenu>) -> Self {
        if config.manual_timezone.is_empty() {
            config.manual_timezone = config.timezone_offset.clone();
        }
        let detected =
            mnema_infra::system_timezone::detect().and_then(timezone::validate_system_timezone);
        if config.timezone_mode == TimezoneMode::Automatic
            && let Ok(name) = &detected
        {
            config.timezone_offset.clone_from(name);
        }
        let timezone_error = detected.as_ref().err().map(ToString::to_string);
        let detected_timezone = detected.ok();
        let dark_mode = config.dark_mode;
        let native_menu_synced_dark_mode = native_menu.as_ref().map(|_| dark_mode);

        let timezone = parse_timezone_offset(&config.timezone_offset).unwrap_or(default_timezone());
        let now = OffsetDateTime::now_utc().to_offset(timezone);
        let today = now.date().to_string();
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
            timezone_offset: config.timezone_offset,
            timezone_mode: config.timezone_mode,
            applied_timezone_mode: config.timezone_mode,
            manual_timezone: config.manual_timezone,
            detected_timezone,
            timezone_error,
            availability_start: config.availability_start,
            availability_end: config.availability_end,
            sleep_start: "23:00".into(),
            sleep_end: "07:00".into(),
            travel_buffer_minutes: "15".into(),
            view: View::Home,
            last_view: View::Settings,
            quick_capture: String::new(),
            assistant_input: String::new(),
            assistant_messages: vec![AssistantChatMessage {
                role: "Assistant",
                content:
                    "タスクを追加したり、次に取り組むことを確認できます。例: 企画書を作る /due tomorrow /minutes 45"
                        .into(),
            }],
            editing_schedule_block_id: None,
            schedule_edit_start: String::new(),
            schedule_edit_end: String::new(),
            target_date: today.clone(),
            scroll_home_agenda_to_now: true,
            home_filter: home::TaskFilter::All,
            home_calendar_mode: ScheduleViewMode::Day,
            workspace_ui: pages::WorkspaceUi {
                read_notifications: config.read_notifications.into_iter().collect(),
                task_grouping: config.task_grouping,
                ..Default::default()
            },
            tasks: Vec::new(),
            done_tasks: Vec::new(),
            lists: Vec::new(),
            statuses: Vec::new(),
            status_groups: Vec::new(),
            confirming_delete_task_id: None,
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
            project_view_mode: ProjectViewMode::Details,
            plan: None,
            auto_preview: None,
            planning_days: 1,
            schedule: Vec::new(),
            schedule_month: Vec::new(),
            automation_logs: Vec::new(),
            scheduling_preferences: None,
            habits: Vec::new(),
            habit_occurrences: Vec::new(),
            habit_draft: HabitDraft::default(),
            calendar_accounts: Vec::new(),
            calendar_sync_cursors: Vec::new(),
            calendar_selection_edits: HashMap::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            message: String::new(),
            error: None,
            dark_mode,
            home_task_density: config.home_task_density,
            home_task_sort: config.home_task_sort,
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
                self.undo_stack.clear();
                self.redo_stack.clear();
                self.calendar_selection_edits.clear();
                self.workspace_ui.task_editor = None;
                self.workspace_ui.detail_task_id = None;
                self.workspace_ui.capture_list = None;
                self.refresh_tasks();
                self.refresh_projects();
                self.refresh_task_context();
                self.refresh_schedule();
                self.refresh_schedule_month();
                self.refresh_automation_logs();
                self.refresh_scheduling_preferences();
                if self.applied_timezone_mode == TimezoneMode::Automatic {
                    self.refresh_system_timezone(true);
                }
                self.refresh_habits();
                self.refresh_calendar_accounts();
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
            tasks.retain(|task| task.deleted_at.is_none());

            let mut active_tasks = Vec::new();
            let mut done_tasks = Vec::new();
            for task in tasks {
                if done_status_ids.contains(&task.status_id) {
                    done_tasks.push(task);
                } else {
                    active_tasks.push(task);
                }
            }
            active_tasks.sort_by_key(|task| (task.due_date, task.created_at));
            done_tasks.sort_by_key(|task| Reverse(task.updated_at));
            Result::<(Vec<Task>, Vec<Task>)>::Ok((active_tasks, done_tasks))
        });

        match result {
            Ok((tasks, done_tasks)) => {
                self.tasks = tasks;
                self.done_tasks = done_tasks;
                self.auto_preview = None;
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
                if let Some(selected) = &self.selected_project_id
                    && !projects.iter().any(|project| &project.id == selected)
                {
                    self.selected_project_id = None;
                    self.project_lists.clear();
                    self.milestones.clear();
                }
                self.projects = projects;
                self.error = None;
                self.refresh_project_children();
            }
            Err(error) => self.set_error(error),
        }
    }

    fn refresh_task_context(&mut self) {
        let Ok(vault) = self.vault_clone() else {
            return;
        };
        let project_ids = self
            .projects
            .iter()
            .map(|project| project.id.clone())
            .collect::<Vec<_>>();

        let result = self.runtime.block_on(async move {
            let list_repo = vault.list_repo();
            let status_repo = vault.status_repo();
            let mut lists = list_repo.list_system().await?;
            let mut milestones = Vec::new();
            for project_id in &project_ids {
                lists.extend(list_repo.list_by_project(project_id.clone()).await?);
                milestones.extend(
                    vault
                        .milestone_repo()
                        .list_by_project(project_id.clone())
                        .await?,
                );
            }
            lists.sort_by(|left, right| left.name.cmp(&right.name));

            let status_groups = status_repo.list_groups().await?;
            let mut statuses = status_repo.list_statuses_for_project(None).await?;
            for project_id in project_ids {
                statuses.extend(
                    status_repo
                        .list_statuses_for_project(Some(project_id))
                        .await?,
                );
            }
            statuses.sort_by(|left, right| {
                left.project_id
                    .is_some()
                    .cmp(&right.project_id.is_some())
                    .then_with(|| left.order.cmp(&right.order))
                    .then_with(|| left.name.cmp(&right.name))
            });

            Result::<(Vec<List>, Vec<Status>, Vec<StatusGroup>, Vec<Milestone>)>::Ok((
                lists,
                statuses,
                status_groups,
                milestones,
            ))
        });

        match result {
            Ok((lists, statuses, status_groups, milestones)) => {
                self.lists = lists;
                self.statuses = statuses;
                self.status_groups = status_groups;
                self.workspace_ui.task_milestones = milestones;
                self.error = None;
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

    fn persist_task_status(
        &mut self,
        task_id: TaskId,
        status_id: StatusId,
    ) -> Result<(Task, StatusId)> {
        let Ok(vault) = self.vault_clone() else {
            return Err(anyhow!("vault is not connected"));
        };
        self.runtime.block_on(async move {
            let task_repo = vault.task_repo();
            let mut task = task_repo
                .find(task_id)
                .await?
                .filter(|task| task.deleted_at.is_none())
                .ok_or_else(|| anyhow!("task not found"))?;
            let previous_status_id = task.status_id.clone();
            if previous_status_id == status_id {
                return Result::<(Task, StatusId)>::Ok((task, previous_status_id));
            }
            task.status_id = status_id;
            task.updated_at = OffsetDateTime::now_utc();
            task_repo.update(task.clone()).await?;
            Result::<(Task, StatusId)>::Ok((task, previous_status_id))
        })
    }

    fn update_task_status(&mut self, task_id: TaskId, status_id: StatusId) {
        let result = self.persist_task_status(task_id, status_id);

        match result {
            Ok((task, previous_status_id)) => {
                if previous_status_id != task.status_id {
                    self.undo_stack.push(StatusHistoryEntry {
                        task_id: task.id.clone(),
                        task_title: task.title.clone(),
                        before_status_id: previous_status_id,
                        after_status_id: task.status_id.clone(),
                    });
                    self.redo_stack.clear();
                }
                self.message = format!("Updated status: {}", task.title);
                self.error = None;
                self.refresh_tasks();
                self.refresh_schedule();
                self.refresh_schedule_month();
            }
            Err(error) => self.set_error(error),
        }
    }

    fn undo_status_change(&mut self) {
        let Some(entry) = self.undo_stack.pop() else {
            self.message = String::from("Nothing to undo");
            return;
        };

        match self.persist_task_status(entry.task_id.clone(), entry.before_status_id.clone()) {
            Ok((_task, _)) => {
                self.message = format!("Undid status: {}", entry.task_title);
                self.error = None;
                self.redo_stack.push(entry);
                self.refresh_tasks();
                self.refresh_schedule();
                self.refresh_schedule_month();
            }
            Err(error) => {
                self.undo_stack.push(entry);
                self.set_error(error);
            }
        }
    }

    fn redo_status_change(&mut self) {
        let Some(entry) = self.redo_stack.pop() else {
            self.message = String::from("Nothing to redo");
            return;
        };

        match self.persist_task_status(entry.task_id.clone(), entry.after_status_id.clone()) {
            Ok((_task, _)) => {
                self.message = format!("Redid status: {}", entry.task_title);
                self.error = None;
                self.undo_stack.push(entry);
                self.refresh_tasks();
                self.refresh_schedule();
                self.refresh_schedule_month();
            }
            Err(error) => {
                self.redo_stack.push(entry);
                self.set_error(error);
            }
        }
    }

    fn delete_task(&mut self, task_id: TaskId) {
        let Ok(vault) = self.vault_clone() else {
            return;
        };
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
                self.refresh_tasks();
                self.refresh_schedule();
            }
            Err(error) => self.set_error(error),
        }
    }

    fn handle_task_action(&mut self, action: TaskAction) {
        match action {
            TaskAction::SetStatus(task_id, status_id) => {
                self.update_task_status(task_id, status_id)
            }
            TaskAction::RequestDelete(task_id) => self.confirming_delete_task_id = Some(task_id),
            TaskAction::ConfirmDelete(task_id) => {
                self.confirming_delete_task_id = None;
                self.delete_task(task_id);
            }
        }
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

        match parse_quick_capture(&input, self.now_in_timezone().date()) {
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
        let timezone = self.app_timezone();
        let schedule = self
            .schedule
            .iter()
            .take(10)
            .map(|block| {
                format!(
                    "- {}-{} {} [{}]",
                    format_hm_in(block.start_at, timezone),
                    format_hm_in(block.end_at, timezone),
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
        let end_date = match target_date.next_day() {
            Some(date) => date,
            None => {
                self.set_error(anyhow!("日付範囲を作成できません"));
                return;
            }
        };
        let planning_window =
            match mnema_app::iana_date_range(target_date, end_date, self.timezone_offset.trim()) {
                Ok(window) => window,
                Err(error) => {
                    self.set_error(anyhow!(error.to_string()));
                    return;
                }
            };

        let result = self.runtime.block_on(async move {
            let schedule_block_repo = vault.schedule_block_repo();
            Result::<Vec<ScheduleBlock>>::Ok(
                schedule_block_repo
                    .list_overlapping(planning_window.start, planning_window.end)
                    .await?,
            )
        });

        match result {
            Ok(schedule) => {
                self.schedule = schedule;
                self.error = None;
            }
            Err(error) => self.set_error(error),
        }
    }

    fn refresh_schedule_month(&mut self) {
        let result = (|| -> Result<_> {
            let vault = self.vault_clone()?;
            let date = parse_required_date(&self.target_date)?;
            let start = calendar_grid_start(first_day_of_month(date)?);
            let end = add_days(start, 49).ok_or_else(|| anyhow!("日付範囲を作成できません"))?;
            let window = mnema_app::iana_date_range(start, end, self.timezone_offset.trim())?;
            let today = self.now_in_timezone().date();
            let today_start = mnema_app::iana_date_range(
                today,
                today.next_day().unwrap(),
                self.timezone_offset.trim(),
            )?
            .start;
            self.runtime.block_on(async move {
                let repo = vault.schedule_block_repo();
                let mut blocks = repo.list_overlapping(window.start, window.end).await?;
                blocks.sort_by_key(|block| (block.start_at, block.end_at));
                let future = repo
                    .list_overlapping(today_start, Date::MAX.midnight().assume_utc())
                    .await?;
                let external = vault
                    .external_event_repo()
                    .list_overlapping(window.start, window.end)
                    .await?;
                Result::<_>::Ok((blocks, future, external))
            })
        })();
        match result {
            Ok((blocks, future, external)) => {
                self.schedule_month = blocks;
                self.workspace_ui.calendar.future_blocks = future;
                self.workspace_ui.calendar.external_events = external;
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

    fn refresh_scheduling_preferences(&mut self) {
        let Ok(vault) = self.vault_clone() else {
            return;
        };
        let timezone = normalized_iana_timezone(&self.timezone_offset);
        let result = self.runtime.block_on(async move {
            let repo = vault.scheduling_preferences_repo();
            let user_id = local_user_id();
            if let Some(preferences) = repo.get_for_user(user_id.clone()).await? {
                return Result::<SchedulingPreferences>::Ok(preferences);
            }
            let preferences = mnema_app::default_scheduling_preferences(
                user_id,
                timezone,
                OffsetDateTime::now_utc(),
            );
            repo.upsert(preferences.clone()).await?;
            Result::<SchedulingPreferences>::Ok(preferences)
        });

        match result {
            Ok(preferences) => {
                let mut form = SchedulingForm::from_legacy(
                    self.timezone_offset.clone(),
                    self.availability_start.clone(),
                    self.availability_end.clone(),
                );
                form.apply_preferences(&preferences);
                if self.timezone_mode == TimezoneMode::Manual {
                    self.timezone_offset = form.timezone;
                    self.manual_timezone.clone_from(&self.timezone_offset);
                }
                self.availability_start = form.work_start;
                self.availability_end = form.work_end;
                self.sleep_start = form.sleep_start;
                self.sleep_end = form.sleep_end;
                self.travel_buffer_minutes = form.travel_buffer_minutes;
                self.scheduling_preferences = Some(preferences);
                self.error = None;
            }
            Err(error) => self.set_error(error),
        }
    }

    fn save_scheduling_preferences(&mut self) -> Result<()> {
        let form = SchedulingForm {
            timezone: self.timezone_offset.clone(),
            work_start: self.availability_start.clone(),
            work_end: self.availability_end.clone(),
            sleep_start: self.sleep_start.clone(),
            sleep_end: self.sleep_end.clone(),
            travel_buffer_minutes: self.travel_buffer_minutes.clone(),
        };
        let preferences = form.build_preferences(
            self.scheduling_preferences.as_ref(),
            local_user_id(),
            OffsetDateTime::now_utc(),
        )?;
        let vault = self.vault_clone()?;
        let saved = preferences.clone();
        self.runtime.block_on(async move {
            vault
                .scheduling_preferences_repo()
                .upsert(preferences)
                .await?;
            Result::<()>::Ok(())
        })?;
        self.scheduling_preferences = Some(saved);
        self.auto_preview = None;
        Ok(())
    }

    fn refresh_habits(&mut self) {
        let Ok(vault) = self.vault_clone() else {
            return;
        };
        let start = self.now_in_timezone().date();
        let Some(end) = add_days(start, 7) else {
            self.set_error(anyhow!("Habitの日付範囲を作成できません"));
            return;
        };
        let result = self.runtime.block_on(async move {
            let habits = vault.habit_repo();
            let occurrences = vault.habit_occurrence_repo();
            let service = HabitService::new(habits.as_ref(), occurrences.as_ref());
            let habits = service.list(local_user_id()).await?;
            let occurrences = service
                .expand_occurrences(local_user_id(), start, end)
                .await?;
            Result::<(Vec<Habit>, Vec<HabitOccurrence>)>::Ok((habits, occurrences))
        });
        match result {
            Ok((habits, occurrences)) => {
                self.habits = habits;
                self.habit_occurrences = occurrences;
                self.error = None;
            }
            Err(error) => self.set_error(error),
        }
    }

    fn add_habit(&mut self) {
        let request = match self.habit_draft.request(local_user_id()) {
            Ok(request) => request,
            Err(error) => {
                self.set_error(error);
                return;
            }
        };
        let Ok(vault) = self.vault_clone() else {
            return;
        };
        let result = self.runtime.block_on(async move {
            let habits = vault.habit_repo();
            let occurrences = vault.habit_occurrence_repo();
            HabitService::new(habits.as_ref(), occurrences.as_ref())
                .add(request)
                .await
                .map_err(Into::into)
        });
        match result {
            Ok(habit) => {
                self.habit_draft.clear_after_add();
                self.message = format!("Habitを追加しました: {}", habit.title);
                self.auto_preview = None;
                self.refresh_habits();
            }
            Err(error) => self.set_error(error),
        }
    }

    fn disable_habit(&mut self, habit_id: HabitId) {
        let Ok(vault) = self.vault_clone() else {
            return;
        };
        let result = self.runtime.block_on(async move {
            let habits = vault.habit_repo();
            let occurrences = vault.habit_occurrence_repo();
            HabitService::new(habits.as_ref(), occurrences.as_ref())
                .disable(habit_id)
                .await
                .map_err(Into::into)
        });
        match result {
            Ok(habit) => {
                self.message = format!("Habitを無効化しました: {}", habit.title);
                self.auto_preview = None;
                self.refresh_habits();
            }
            Err(error) => self.set_error(error),
        }
    }

    fn skip_habit_occurrence(&mut self, occurrence_id: HabitOccurrenceId) {
        let Ok(vault) = self.vault_clone() else {
            return;
        };
        let result = self.runtime.block_on(async move {
            let habits = vault.habit_repo();
            let occurrences = vault.habit_occurrence_repo();
            HabitService::new(habits.as_ref(), occurrences.as_ref())
                .skip(occurrence_id, Some("Skipped from desktop".into()))
                .await
                .map_err(Into::into)
        });
        match result {
            Ok(_) => {
                self.message = "Occurrenceをスキップしました".into();
                self.auto_preview = None;
                self.refresh_habits();
            }
            Err(error) => self.set_error(error),
        }
    }

    fn snooze_habit_occurrence(&mut self, occurrence_id: HabitOccurrenceId) {
        let Ok(vault) = self.vault_clone() else {
            return;
        };
        let until = OffsetDateTime::now_utc() + time::Duration::hours(24);
        let result = self.runtime.block_on(async move {
            let habits = vault.habit_repo();
            let occurrences = vault.habit_occurrence_repo();
            HabitService::new(habits.as_ref(), occurrences.as_ref())
                .snooze(occurrence_id, until)
                .await
                .map_err(Into::into)
        });
        match result {
            Ok(_) => {
                self.message = "Occurrenceを24時間snoozeしました".into();
                self.auto_preview = None;
                self.refresh_habits();
            }
            Err(error) => self.set_error(error),
        }
    }

    fn refresh_calendar_accounts(&mut self) {
        let Ok(vault) = self.vault_clone() else {
            return;
        };
        let result = self.runtime.block_on(async move {
            let account_repo = vault.calendar_account_repo();
            let cursor_repo = vault.calendar_sync_cursor_repo();
            let accounts = account_repo.list_enabled(local_user_id()).await?;
            let mut cursors = Vec::new();
            for account in &accounts {
                let mut calendar_ids = account.selected_calendar_ids.clone();
                if let Some(managed) = &account.managed_calendar_id
                    && !calendar_ids.contains(managed)
                {
                    calendar_ids.push(managed.clone());
                }
                for calendar_id in calendar_ids {
                    if let Some(cursor) = cursor_repo.get(account.id.clone(), calendar_id).await? {
                        cursors.push(cursor);
                    }
                }
            }
            Result::<(Vec<CalendarAccount>, Vec<CalendarSyncCursor>)>::Ok((accounts, cursors))
        });
        match result {
            Ok((accounts, cursors)) => {
                self.calendar_selection_edits
                    .retain(|id, _| accounts.iter().any(|account| &account.id == id));
                for account in &accounts {
                    let previous = self
                        .calendar_accounts
                        .iter()
                        .find(|old| old.id == account.id)
                        .map(|old| {
                            normalize_selected_calendar_ids(
                                &old.selected_calendar_ids.join(","),
                                old.managed_calendar_id.as_deref(),
                            )
                            .join(", ")
                        });
                    let edited = self
                        .calendar_selection_edits
                        .get(&account.id)
                        .is_some_and(|draft| previous.as_ref().is_some_and(|old| draft != old));
                    if !edited {
                        let selected = normalize_selected_calendar_ids(
                            &account.selected_calendar_ids.join(","),
                            account.managed_calendar_id.as_deref(),
                        );
                        self.calendar_selection_edits
                            .insert(account.id.clone(), selected.join(", "));
                    }
                }
                self.calendar_accounts = accounts;
                self.calendar_sync_cursors = cursors;
                self.error = None;
            }
            Err(error) => self.set_error(error),
        }
    }

    fn save_calendar_selection(&mut self, account_id: CalendarAccountId) {
        let Some(mut account) = self
            .calendar_accounts
            .iter()
            .find(|account| account.id == account_id)
            .cloned()
        else {
            self.set_error(anyhow!("Calendar accountが見つかりません"));
            return;
        };
        let input = self
            .calendar_selection_edits
            .get(&account_id)
            .cloned()
            .unwrap_or_default();
        account.selected_calendar_ids =
            normalize_selected_calendar_ids(&input, account.managed_calendar_id.as_deref());
        account.updated_at = OffsetDateTime::now_utc();
        let saved_count = account.selected_calendar_ids.len();
        let Ok(vault) = self.vault_clone() else {
            return;
        };
        let result = self.runtime.block_on(async move {
            vault.calendar_account_repo().upsert(account).await?;
            Result::<()>::Ok(())
        });
        match result {
            Ok(()) => {
                self.message = format!("Busy calendarを{saved_count}件保存しました");
                self.error = None;
                self.auto_preview = None;
                self.calendar_selection_edits.remove(&account_id);
                self.refresh_calendar_accounts();
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
                self.refresh_schedule_month();
            }
            Err(error) => self.set_error(error),
        }
    }

    fn start_edit_schedule_block(&mut self, block_id: ScheduleBlockId) {
        let Some(block) = self.schedule.iter().find(|block| block.id == block_id) else {
            self.set_error(anyhow!("schedule block not found"));
            return;
        };
        let timezone = self.app_timezone();
        self.workspace_ui.calendar.edit_start_date =
            block.start_at.to_offset(timezone).date().to_string();
        self.workspace_ui.calendar.edit_end_date =
            block.end_at.to_offset(timezone).date().to_string();
        self.editing_schedule_block_id = Some(block.id.clone());
        self.schedule_edit_start = format_hm_in(block.start_at, timezone);
        self.schedule_edit_end = format_hm_in(block.end_at, timezone);
        self.error = None;
    }

    fn save_schedule_block_edit(&mut self) {
        let Some(block_id) = self.editing_schedule_block_id.clone() else {
            return;
        };
        let parsed = (|| -> Result<_> {
            let start_date = parse_required_date(&self.workspace_ui.calendar.edit_start_date)?;
            let end_date = parse_required_date(&self.workspace_ui.calendar.edit_end_date)?;
            let start = calendar::parse_event_time(
                start_date,
                &self.schedule_edit_start,
                &self.timezone_offset,
                true,
            )?;
            let end = calendar::parse_event_time(
                end_date,
                &self.schedule_edit_end,
                &self.timezone_offset,
                false,
            )?;
            Ok((start, end))
        })();
        let (start_at, end_at) = match parsed {
            Ok(window) => window,
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
                self.refresh_schedule_month();
            }
            Err(error) => self.set_error(error),
        }
    }

    fn clear_schedule_block_editor(&mut self) {
        self.editing_schedule_block_id = None;
        self.schedule_edit_start.clear();
        self.schedule_edit_end.clear();
    }

    fn capture_task(&mut self, request: CaptureTaskRequest) {
        self.capture_task_in_location(request, None, None);
    }

    fn capture_task_in_location(
        &mut self,
        request: CaptureTaskRequest,
        project_id: Option<ProjectId>,
        list_id: Option<ListId>,
    ) {
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
            let task = if project_id.is_some() || list_id.is_some() {
                let list = if let Some(id) = list_id {
                    list_repo
                        .find(id)
                        .await?
                        .ok_or_else(|| anyhow!("リストが見つかりません"))?
                } else {
                    let id = project_id.clone().expect("project or list is set");
                    let lists = list_repo.list_by_project(id.clone()).await?;
                    if let Some(list) = lists.first() {
                        list.clone()
                    } else {
                        let list = List {
                            id: ListId::new(),
                            project_id: Some(id),
                            name: "タスク".into(),
                            is_system: false,
                            kind: ListKind::Project,
                            view_type: ListViewType::List,
                            order: 0,
                        };
                        list_repo.insert(list.clone()).await?;
                        list
                    }
                };
                service.capture_task_in_list(request, list.id).await?.task
            } else {
                service.capture_inbox_task(request).await?.task
            };
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
                self.message = format!("Added: {}", task.title);
                self.error = None;
                self.refresh_tasks();
                self.refresh_automation_logs();
                self.refresh_task_context();
                self.refresh_project_children();
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
        let start_date = match parse_optional_date_with_today(
            &self.project_start_date,
            self.now_in_timezone().date(),
        ) {
            Ok(date) => date,
            Err(error) => {
                self.set_error(error);
                return;
            }
        };
        let end_date = match parse_optional_date_with_today(
            &self.project_end_date,
            self.now_in_timezone().date(),
        ) {
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
                self.refresh_task_context();
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
                self.refresh_task_context();
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

    fn preview_auto_schedule(&mut self) {
        let Ok(vault) = self.vault_clone() else {
            return;
        };
        let start_date = match parse_required_date(&self.target_date) {
            Ok(date) => date,
            Err(error) => {
                self.set_error(error);
                return;
            }
        };
        let end_date_exclusive = match date_range_end(start_date, self.planning_days) {
            Ok(date) => date,
            Err(error) => {
                self.set_error(error);
                return;
            }
        };
        let request = AutoScheduleRequest {
            user_id: local_user_id(),
            start_date,
            end_date_exclusive,
            timezone: self.timezone_offset.trim().to_owned(),
            named_hours: vec!["work".into()],
            not_before: None,
        };
        let result = self.runtime.block_on(async move {
            let tasks = vault.task_repo();
            let statuses = vault.status_repo();
            let blocks = vault.schedule_block_repo();
            let events = vault.external_event_repo();
            let habits = vault.habit_repo();
            let occurrences = vault.habit_occurrence_repo();
            let preferences = vault.scheduling_preferences_repo();
            AutoScheduleService::new(
                tasks.as_ref(),
                statuses.as_ref(),
                blocks.as_ref(),
                events.as_ref(),
                habits.as_ref(),
                occurrences.as_ref(),
                preferences.as_ref(),
            )
            .preview(request)
            .await
            .map_err(Into::into)
        });
        match result {
            Ok(preview) => {
                let proposed = preview.output.blocks.len();
                let unscheduled = preview.output.unscheduled.len();
                let changed = preview.diff.changed_count();
                self.plan = None;
                self.message = format!(
                    "Preview: {proposed}件配置 / {unscheduled}件未配置 / {changed}件変更（未保存）"
                );
                self.auto_preview = Some(preview);
                self.error = None;
            }
            Err(error) => self.set_error(error),
        }
    }

    fn apply_auto_schedule(&mut self) {
        let Some(preview) = self.auto_preview.clone() else {
            self.set_error(anyhow!("先にPreviewを作成してください"));
            return;
        };
        let Ok(vault) = self.vault_clone() else {
            return;
        };
        let fingerprint = preview.diff.fingerprint.clone();
        let result = self.runtime.block_on(async move {
            let tasks = vault.task_repo();
            let statuses = vault.status_repo();
            let blocks = vault.schedule_block_repo();
            let events = vault.external_event_repo();
            let habits = vault.habit_repo();
            let occurrences = vault.habit_occurrence_repo();
            let preferences = vault.scheduling_preferences_repo();
            AutoScheduleService::new(
                tasks.as_ref(),
                statuses.as_ref(),
                blocks.as_ref(),
                events.as_ref(),
                habits.as_ref(),
                occurrences.as_ref(),
                preferences.as_ref(),
            )
            .apply(&preview, &fingerprint)
            .await
            .map_err(Into::into)
        });
        match result {
            Ok(applied) => {
                self.message =
                    format!("{}件の自動スケジュールを保存しました", applied.blocks.len());
                self.auto_preview = None;
                self.error = None;
                self.refresh_schedule();
                self.refresh_schedule_month();
                self.refresh_habits();
            }
            Err(error) => self.set_error(error),
        }
    }

    fn create_manual_schedule_block(&mut self, request: ManualScheduleRequest) {
        let Ok(vault) = self.vault_clone() else {
            return;
        };
        let duration = time::Duration::minutes(i64::from(request.duration_minutes.max(1)));
        let now = OffsetDateTime::now_utc();
        let title = request.title.clone();
        let block = ScheduleBlock {
            id: ScheduleBlockId::new(),
            task_id: Some(request.task_id),
            habit_occurrence_id: None,
            title_snapshot: Some(request.title),
            start_at: request.start_at,
            end_at: request.start_at + duration,
            block_type: ScheduleBlockType::Task,
            state: ScheduleBlockState::Scheduled,
            locked: true,
            source: ScheduleBlockSource::Manual,
            required_minutes: Some(request.duration_minutes.max(1)),
            created_at: now,
            updated_at: now,
        };

        let result = self.runtime.block_on(async move {
            vault.schedule_block_repo().insert(block).await?;
            Result::<()>::Ok(())
        });

        match result {
            Ok(()) => {
                self.plan = None;
                self.message = format!("Scheduled manually: {title}");
                self.error = None;
                self.refresh_schedule();
                self.refresh_schedule_month();
            }
            Err(error) => self.set_error(error),
        }
    }

    fn move_schedule_block(&mut self, request: MoveScheduleBlockRequest) {
        let Ok(vault) = self.vault_clone() else {
            return;
        };
        let duration_minutes = request.duration_minutes.max(1);
        let duration = time::Duration::minutes(duration_minutes);
        let title = request.title.clone();
        let block_id = request.block_id;
        let start_at = request.start_at;
        let end_at = start_at + duration;

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
                self.plan = None;
                self.message = format!(
                    "Moved manually: {}",
                    block.title_snapshot.as_deref().unwrap_or(title.as_str())
                );
                self.error = None;
                self.refresh_schedule();
                self.refresh_schedule_month();
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

    fn app_timezone(&self) -> UtcOffset {
        parse_timezone_offset(&self.timezone_offset).unwrap_or(default_timezone())
    }

    fn now_in_timezone(&self) -> OffsetDateTime {
        OffsetDateTime::now_utc().to_offset(self.app_timezone())
    }

    fn selected_project_title(&self) -> Option<String> {
        let selected_id = self.selected_project_id.as_ref()?;
        self.projects
            .iter()
            .find(|project| &project.id == selected_id)
            .map(|project| project.title.clone())
    }

    fn save_settings(&mut self) {
        if self.timezone_mode == TimezoneMode::Automatic {
            self.refresh_system_timezone(false);
            if let Some(error) = &self.timezone_error {
                self.set_error(anyhow!(error.clone()));
                return;
            }
        } else {
            self.manual_timezone.clone_from(&self.timezone_offset);
        }
        if let Err(error) = parse_timezone_offset(&self.timezone_offset) {
            self.set_error(error);
            return;
        }
        if let Err(error) =
            validate_availability_window(&self.availability_start, &self.availability_end)
        {
            self.set_error(error);
            return;
        }
        if self.vault.is_some()
            && let Err(error) = self.save_scheduling_preferences()
        {
            self.set_error(error);
            return;
        }
        let config = DesktopConfig {
            vault_path: self.normalized_vault_path().display().to_string(),
            storage_backend: self.selected_backend,
            sqlite_path: self.normalized_sqlite_path().display().to_string(),
            database_url: self.normalized_database_url(),
            dark_mode: self.dark_mode,
            timezone_offset: self.timezone_offset.trim().to_string(),
            timezone_mode: self.timezone_mode,
            manual_timezone: self.manual_timezone.trim().to_string(),
            availability_start: self.availability_start.trim().to_string(),
            availability_end: self.availability_end.trim().to_string(),
            home_task_density: self.home_task_density,
            home_task_sort: self.home_task_sort,
            task_grouping: self.workspace_ui.task_grouping,
            llm_provider: self.llm_provider,
            ollama_url: self.ollama_url.trim().to_string(),
            openai_url: self.openai_url.trim().to_string(),
            planning_model: self.planning_model.trim().to_string(),
            routine_model: self.routine_model.trim().to_string(),
            read_notifications: self
                .workspace_ui
                .read_notifications
                .iter()
                .cloned()
                .collect(),
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
                self.timezone_offset = config.timezone_offset;
                self.applied_timezone_mode = config.timezone_mode;
                self.availability_start = config.availability_start;
                self.availability_end = config.availability_end;
                self.home_task_density = config.home_task_density;
                self.home_task_sort = config.home_task_sort;
                self.scroll_home_agenda_to_now = true;
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
            .inspect_err(|error| {
                self.set_error(anyhow!(error.to_string()));
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
            MenuAction::Close => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            MenuAction::Undo => self.undo_status_change(),
            MenuAction::Redo => self.redo_status_change(),
            MenuAction::SetView(view) => self.view = view,
            MenuAction::SetDarkMode(dark_mode) => self.dark_mode = dark_mode,
        }
    }

    fn handle_keyboard_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.input_mut(|input| {
            input.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::CTRL,
                egui::Key::T,
            ))
        }) {
            self.open_task_capture(ctx);
            return;
        }
        if self.workspace_ui.capture.open {
            return;
        }
        if ctx.egui_wants_keyboard_input() {
            return;
        }

        self.calendar_keyboard_shortcuts(ctx);

        let redo = egui::KeyboardShortcut::new(
            egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
            egui::Key::Z,
        );
        let undo = egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::Z);
        if ctx.input_mut(|input| input.consume_shortcut(&redo)) {
            self.redo_status_change();
        } else if ctx.input_mut(|input| input.consume_shortcut(&undo)) {
            self.undo_status_change();
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
        let appearance = (
            self.dark_mode,
            self.home_task_density,
            self.home_task_sort,
            self.workspace_ui.task_grouping,
        );
        self.handle_native_menu(ui.ctx());
        self.handle_keyboard_shortcuts(ui.ctx());

        let dark_factor = if self.dark_mode { 1.0 } else { 0.0 };
        if self.workspace_ui.applied_dark_mode != Some(self.dark_mode) {
            configure_style(ui.ctx(), dark_factor, self.dark_mode);
            self.workspace_ui.applied_dark_mode = Some(self.dark_mode);
        }
        let palette = Palette::at(dark_factor);

        self.show_shell(ui, palette);
        self.show_task_details(ui.ctx(), palette);
        self.show_calendar_event_details(ui.ctx(), palette);
        self.show_task_capture(ui.ctx(), palette);
        self.automatic_refresh(ui.ctx());
        if appearance
            != (
                self.dark_mode,
                self.home_task_density,
                self.home_task_sort,
                self.workspace_ui.task_grouping,
            )
        {
            self.persist_ui_preferences();
        }
    }
}

impl MnemaGuiApp {
    fn show_calendar(&mut self, ui: &mut egui::Ui, palette: Palette) {
        section_header(ui, "Calendar", palette);
        let mut refresh = false;
        ui.horizontal(|ui| {
            ui.label(
                regular_text("接続カレンダーは予定のhard busyとして自動計画へ反映されます。")
                    .color(palette.muted),
            );
            if ui.button("Refresh status").clicked() {
                refresh = true;
            }
        });
        ui.add_space(10.0);

        if self.calendar_accounts.is_empty() {
            egui::Frame::new()
                .fill(palette.surface)
                .stroke(Stroke::new(1.0_f32, palette.border))
                .corner_radius(10.0)
                .inner_margin(16.0)
                .show(ui, |ui| {
                    ui.label(bold_text("No calendar connected").color(palette.section));
                    ui.label(
                        regular_text("Google OAuth接続はWebサーバーのSettingsから開始できます。")
                            .color(palette.muted),
                    );
                    ui.horizontal(|ui| {
                        ui.monospace("http://127.0.0.1:8080/#settings");
                        if ui.button("Copy Web URL").clicked() {
                            ui.ctx().copy_text("http://127.0.0.1:8080/#settings".into());
                        }
                    });
                });
        } else {
            let accounts = self.calendar_accounts.clone();
            let mut save_account_id = None;
            for account in &accounts {
                let selected_calendar_ids = normalize_selected_calendar_ids(
                    &account.selected_calendar_ids.join(","),
                    account.managed_calendar_id.as_deref(),
                );
                egui::Frame::new()
                    .fill(palette.surface)
                    .stroke(Stroke::new(1.0_f32, palette.border))
                    .corner_radius(10.0)
                    .inner_margin(14.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(bold_text(&account.display_name).color(palette.section));
                            if let Some(email) = &account.email {
                                ui.label(regular_text(email).color(palette.muted));
                            }
                            ui.label(
                                regular_text(format!(
                                    "{:?} / {:?}",
                                    account.provider, account.access_mode
                                ))
                                .color(palette.muted),
                            );
                        });
                        ui.label(format!(
                            "Timezone: {}",
                            account.timezone.as_deref().unwrap_or("not reported")
                        ));
                        ui.label(format!(
                            "Managed calendar: {}",
                            account
                                .managed_calendar_id
                                .as_deref()
                                .unwrap_or("not selected")
                        ));
                        ui.add_space(6.0);
                        ui.label(bold_text("Busy calendars").size(13.0));
                        ui.label(
                            regular_text(
                                "予定をhard busyとして扱うcalendar IDをカンマ区切りで入力します。Managed calendarは自動的に除外されます。",
                            )
                            .color(palette.muted),
                        );
                        ui.horizontal(|ui| {
                            let editor = self
                                .calendar_selection_edits
                                .entry(account.id.clone())
                                .or_insert_with(|| selected_calendar_ids.join(", "));
                            ui.add_sized(
                                [460.0, INPUT_HEIGHT],
                                text_field(editor, "primary, team@example.com"),
                            );
                            if ui.button("Save calendars").clicked() {
                                save_account_id = Some(account.id.clone());
                            }
                        });
                        ui.add_space(4.0);
                        ui.label(bold_text("Sync status").size(13.0));
                        if selected_calendar_ids.is_empty() {
                            ui.label(regular_text("No calendars selected").color(palette.warning));
                        }
                        for calendar_id in &selected_calendar_ids {
                            ui.horizontal(|ui| {
                                ui.label(format!("• {calendar_id}"));
                                let cursor = self.calendar_sync_cursors.iter().find(|cursor| {
                                    cursor.account_id == account.id
                                        && cursor.calendar_id == *calendar_id
                                });
                                ui.label(regular_text(calendar_sync_label(cursor)).color(
                                    if cursor.is_some() {
                                        palette.success
                                    } else {
                                        palette.muted
                                    },
                                ));
                            });
                        }
                    });
                ui.add_space(8.0);
            }
            if let Some(account_id) = save_account_id {
                self.save_calendar_selection(account_id);
                refresh = false;
            }
        }
        if refresh {
            self.refresh_calendar_accounts();
        }
    }

    fn show_schedule_block_editor(&mut self, ui: &mut egui::Ui, palette: Palette) {
        if self.editing_schedule_block_id.is_none() {
            return;
        }

        ui.add_space(8.0);
        ui.separator();
        ui.label(bold_text("日時を編集").color(palette.section));
        ui.horizontal_wrapped(|ui| {
            ui.label("開始");
            ui.add_sized(
                [125.0, 30.0],
                text_field(
                    &mut self.workspace_ui.calendar.edit_start_date,
                    "YYYY-MM-DD",
                ),
            );
            ui.add_sized(
                [72.0, 30.0],
                text_field(&mut self.schedule_edit_start, "09:00"),
            );
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("終了");
            ui.add_sized(
                [125.0, 30.0],
                text_field(&mut self.workspace_ui.calendar.edit_end_date, "YYYY-MM-DD"),
            );
            ui.add_sized(
                [72.0, 30.0],
                text_field(&mut self.schedule_edit_end, "10:00"),
            );
        });
        let mut save = false;
        let mut cancel = false;
        ui.horizontal(|ui| {
            save = components::button(ui, "日時を保存", true, palette).clicked();
            cancel = components::button(ui, "キャンセル", false, palette).clicked();
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
    error: Color32,
}

impl Palette {
    fn at(dark_factor: f32) -> Self {
        let t = dark_factor.clamp(0.0, 1.0);
        Self {
            text: mix_color(
                Color32::from_rgb(40, 42, 45),
                Color32::from_rgb(235, 235, 238),
                t,
            ),
            muted: mix_color(
                Color32::from_rgb(118, 119, 126),
                Color32::from_rgb(153, 155, 164),
                t,
            ),
            brand: mix_color(
                Color32::from_rgb(58, 96, 78),
                Color32::from_rgb(164, 195, 177),
                t,
            ),
            section: mix_color(
                Color32::from_rgb(58, 96, 78),
                Color32::from_rgb(230, 231, 235),
                t,
            ),
            panel: mix_color(
                Color32::from_rgb(247, 247, 245),
                Color32::from_rgb(29, 30, 33),
                t,
            ),
            surface: mix_color(
                Color32::from_rgb(255, 255, 255),
                Color32::from_rgb(38, 39, 43),
                t,
            ),
            faint: mix_color(
                Color32::from_rgb(239, 240, 238),
                Color32::from_rgb(47, 48, 53),
                t,
            ),
            input: mix_color(
                Color32::from_rgb(255, 255, 255),
                Color32::from_rgb(32, 33, 37),
                t,
            ),
            code_bg: mix_color(
                Color32::from_rgb(238, 242, 246),
                Color32::from_rgb(39, 40, 45),
                t,
            ),
            control_bg: mix_color(
                Color32::from_rgb(241, 242, 240),
                Color32::from_rgb(46, 47, 52),
                t,
            ),
            control_hover: mix_color(
                Color32::from_rgb(230, 237, 233),
                Color32::from_rgb(53, 55, 61),
                t,
            ),
            control_active: mix_color(
                Color32::from_rgb(64, 111, 87),
                Color32::from_rgb(151, 190, 166),
                t,
            ),
            switch_bg: mix_color(
                Color32::from_rgb(224, 231, 239),
                Color32::from_rgb(31, 32, 36),
                t,
            ),
            border: mix_color(
                Color32::from_rgb(225, 226, 223),
                Color32::from_rgb(55, 56, 62),
                t,
            ),
            border_strong: mix_color(
                Color32::from_rgb(204, 207, 203),
                Color32::from_rgb(76, 78, 85),
                t,
            ),
            accent: mix_color(
                Color32::from_rgb(64, 111, 87),
                Color32::from_rgb(151, 190, 166),
                t,
            ),
            selected_fill: mix_color(
                Color32::from_rgb(230, 237, 233),
                Color32::from_rgb(53, 64, 59),
                t,
            ),
            selected_text: mix_color(
                Color32::from_rgb(58, 96, 78),
                Color32::from_rgb(235, 235, 238),
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
    let default_proportional = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let mut text_regular_family = default_proportional.clone();
    let mut text_bold_family = default_proportional;

    if let Some(font_bytes) = load_noto_sans_jp() {
        let regular_name = "noto_sans_jp_regular".to_owned();
        let bold_name = "noto_sans_jp_bold".to_owned();
        fonts.font_data.insert(
            regular_name.clone(),
            Arc::new(FontData::from_owned(font_bytes.clone()).tweak(FontTweak {
                hinting_override: Some(true),
                coords: egui::epaint::text::VariationCoords::new([("wght", FONT_WEIGHT_REGULAR)]),
                ..Default::default()
            })),
        );
        fonts.font_data.insert(
            bold_name.clone(),
            Arc::new(FontData::from_owned(font_bytes).tweak(FontTweak {
                hinting_override: Some(true),
                coords: egui::epaint::text::VariationCoords::new([("wght", FONT_WEIGHT_BOLD)]),
                ..Default::default()
            })),
        );

        if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
            family.insert(0, regular_name.clone());
        }
        if let Some(family) = fonts.families.get_mut(&FontFamily::Monospace) {
            family.push(regular_name.clone());
        }
        text_regular_family.insert(0, regular_name.clone());
        text_bold_family.insert(0, bold_name);
        text_bold_family.push(regular_name);
    }
    fonts.families.insert(
        FontFamily::Name(TEXT_REGULAR_FONT_FAMILY.into()),
        text_regular_family,
    );
    fonts.families.insert(
        FontFamily::Name(TEXT_BOLD_FONT_FAMILY.into()),
        text_bold_family,
    );

    if let Some(font_bytes) = load_montserrat() {
        let font_name = "montserrat".to_owned();
        fonts.font_data.insert(
            font_name.clone(),
            Arc::new(FontData::from_owned(font_bytes).tweak(FontTweak {
                hinting_override: Some(true),
                coords: egui::epaint::text::VariationCoords::new([("wght", FONT_WEIGHT_BOLD)]),
                ..Default::default()
            })),
        );
        fonts
            .families
            .insert(FontFamily::Name(LOGO_FONT_FAMILY.into()), vec![font_name]);
    }

    if let Some(font_bytes) = load_material_icons() {
        let font_name = "material_icons".to_owned();
        fonts.font_data.insert(
            font_name.clone(),
            Arc::new(FontData::from_owned(font_bytes)),
        );
        fonts.families.insert(
            FontFamily::Name(MATERIAL_ICON_FONT_FAMILY.into()),
            vec![font_name.clone()],
        );
        if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
            family.push(font_name);
        }
    }

    ctx.set_fonts(fonts);
}

fn load_noto_sans_jp() -> Option<Vec<u8>> {
    let mut candidates = Vec::new();
    candidates.extend(font_asset_candidates("NotoSansJP-VF.ttf"));
    candidates.extend(font_asset_candidates("NotoSansJP-Regular.ttf"));
    candidates.push(PathBuf::from(r"C:\Windows\Fonts\NotoSansJP-VF.ttf"));
    candidates.push(PathBuf::from(r"C:\Windows\Fonts\NotoSansJP-Regular.ttf"));
    read_first_file(candidates)
}

fn load_montserrat() -> Option<Vec<u8>> {
    read_first_file(font_asset_candidates("Montserrat-VF.ttf"))
}

fn load_material_icons() -> Option<Vec<u8>> {
    read_first_file(font_asset_candidates("MaterialIcons-Regular.ttf"))
}

fn font_asset_candidates(file_name: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(exe_path) = std::env::current_exe()
        && let Some(exe_dir) = exe_path.parent()
    {
        paths.push(exe_dir.join("assets/fonts").join(file_name));
    }
    paths.push(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets/fonts")
            .join(file_name),
    );
    paths
}

fn read_first_file(paths: impl IntoIterator<Item = PathBuf>) -> Option<Vec<u8>> {
    paths.into_iter().find_map(|path| fs::read(path).ok())
}

fn configure_style(ctx: &egui::Context, dark_factor: f32, dark_mode: bool) {
    let palette = Palette::at(dark_factor);
    let mut style = egui::Theme::from_dark_mode(dark_mode).default_style();
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 7.0);
    style
        .text_styles
        .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, egui::FontId::proportional(13.0));
    style.visuals = themed_visuals(palette, dark_mode);
    components::control_style(&mut style);
    ctx.set_global_style(style);
}

fn regular_text(text: impl Into<String>) -> RichText {
    RichText::new(text).family(FontFamily::Name(TEXT_REGULAR_FONT_FAMILY.into()))
}

fn bold_text(text: impl Into<String>) -> RichText {
    RichText::new(text).family(FontFamily::Name(TEXT_BOLD_FONT_FAMILY.into()))
}

fn logo_text(text: impl Into<String>) -> RichText {
    RichText::new(text)
        .family(FontFamily::Name(LOGO_FONT_FAMILY.into()))
        .variation("wght", FONT_WEIGHT_BOLD)
}

fn material_icon_text(icon: char, size: f32, color: Color32) -> RichText {
    RichText::new(icon.to_string())
        .family(FontFamily::Name(MATERIAL_ICON_FONT_FAMILY.into()))
        .size(size)
        .color(color)
}

fn material_icon_font(size: f32) -> egui::FontId {
    egui::FontId::new(size, FontFamily::Name(MATERIAL_ICON_FONT_FAMILY.into()))
}

fn text_field<'a>(value: &'a mut String, hint_text: &'static str) -> TextEdit<'a> {
    TextEdit::singleline(value)
        .margin(egui::Margin::symmetric(9, 4))
        .hint_text(hint_text)
        .vertical_align(Align::Center)
}

fn date_editor(
    ui: &mut egui::Ui,
    value: &mut String,
    timezone: UtcOffset,
    palette: Palette,
) -> bool {
    let current = parse_optional_date(value).ok().flatten();
    let today = OffsetDateTime::now_utc().to_offset(timezone).date();
    let response = components::button(
        ui,
        &current
            .map(|date| date.to_string())
            .unwrap_or_else(|| "日付を設定".into()),
        false,
        palette,
    );
    if let Some(date) = date_picker::show(ui, &response, current, today, "日付", palette) {
        *value = date.map(|date| date.to_string()).unwrap_or_default();
        true
    } else {
        false
    }
}
fn shift_month(value: &mut String, months: i32) {
    let base = parse_optional_date(value)
        .ok()
        .flatten()
        .unwrap_or_else(|| OffsetDateTime::now_utc().date());
    if let Ok(shifted) = add_months(base, months) {
        *value = shifted.to_string();
    }
}

fn minutes_editor(ui: &mut egui::Ui, value: &mut String) {
    let mut minutes = value.trim().parse::<u32>().unwrap_or(0);
    let response = ui.add_sized(
        [92.0, INPUT_HEIGHT],
        egui::DragValue::new(&mut minutes)
            .range(0..=24 * 60)
            .speed(5)
            .suffix(" 分"),
    );
    if response.changed() {
        *value = minutes.to_string();
    }
    if ui.button("クリア").clicked() {
        value.clear();
    }
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
    visuals.selection.stroke = Stroke::new(1.0_f32, palette.selected_text);
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

fn task_status_button(
    ui: &mut egui::Ui,
    id_salt: &'static str,
    task: &Task,
    statuses: &[Status],
    status_groups: &[StatusGroup],
    palette: Palette,
    action: &mut Option<TaskAction>,
) {
    let current_status = statuses.iter().find(|status| status.id == task.status_id);
    let current_kind = current_status
        .and_then(|status| status_group_kind(status, status_groups))
        .unwrap_or(StatusGroupKind::NotStarted);
    let candidates = status_candidates(statuses, task.project_id.as_ref());

    let button_id = ui.make_persistent_id((id_salt, "status_button", task.id.clone()));
    let popup_id = button_id.with("popup");
    let (rect, response) = ui.allocate_exact_size(egui::vec2(30.0, 30.0), egui::Sense::click());
    let response = response.on_hover_text(
        current_status
            .map(|status| status.name.as_str())
            .unwrap_or("Status"),
    );

    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "ステータスを変更")
    });
    paint_status_icon(ui.painter(), rect.center(), &current_kind, palette);

    components::popup(&response)
        .id(popup_id)
        .width(190.0)
        .show(|ui| {
            ui.set_min_width(190.0);
            for status in candidates {
                let (rect, item) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 30.0),
                    egui::Sense::click(),
                );
                if item.hovered() || status.id == task.status_id {
                    ui.painter().rect_filled(rect, 4.0, palette.selected_fill);
                }
                let kind =
                    status_group_kind(status, status_groups).unwrap_or(StatusGroupKind::NotStarted);
                paint_status_icon(
                    ui.painter(),
                    rect.left_center() + egui::vec2(15.0, 0.0),
                    &kind,
                    palette,
                );
                ui.painter().text(
                    rect.left_center() + egui::vec2(34.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    &status.name,
                    egui::FontId::proportional(13.0),
                    palette.text,
                );
                if status.id == task.status_id {
                    ui.painter().text(
                        rect.right_center() - egui::vec2(14.0, 0.0),
                        egui::Align2::CENTER_CENTER,
                        ICON_CHECK,
                        material_icon_font(15.0),
                        palette.text,
                    );
                }
                item.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &status.name)
                });
                if item.clicked() {
                    *action = Some(TaskAction::SetStatus(task.id.clone(), status.id.clone()));
                    egui::Popup::close_id(ui.ctx(), popup_id);
                }
            }
        });
}

fn paint_status_icon(
    painter: &egui::Painter,
    center: egui::Pos2,
    kind: &StatusGroupKind,
    palette: Palette,
) {
    let color = task_status_color(kind, palette);
    let radius = 8.5;
    let stroke = Stroke::new(1.4_f32, color);
    if *kind == StatusGroupKind::NotStarted {
        for segment in 0..9 {
            let points = (0..=4)
                .map(|step| {
                    let angle = (segment as f32 + step as f32 / 6.0) * std::f32::consts::TAU / 9.0;
                    center + egui::vec2(angle.cos(), angle.sin()) * radius
                })
                .collect();
            painter.add(egui::Shape::line(points, stroke));
        }
    } else if *kind == StatusGroupKind::Done {
        painter.circle_filled(center, radius, color);
        painter.text(
            center,
            egui::Align2::CENTER_CENTER,
            ICON_CHECK,
            material_icon_font(14.0),
            Color32::WHITE,
        );
    } else {
        painter.circle_stroke(center, radius, stroke);
        if *kind == StatusGroupKind::InProgress {
            painter.circle_filled(center, 6.0, color);
        } else {
            painter.line_segment(
                [center - egui::vec2(3.0, 0.0), center + egui::vec2(3.0, 0.0)],
                stroke,
            );
        }
    }
}

fn status_candidates<'a>(
    statuses: &'a [Status],
    project_id: Option<&ProjectId>,
) -> Vec<&'a Status> {
    let mut candidates = statuses
        .iter()
        .filter(|status| status.project_id.as_ref() == project_id)
        .collect::<Vec<_>>();
    if project_id.is_some() {
        candidates.extend(statuses.iter().filter(|status| status.project_id.is_none()));
    }
    candidates.sort_by_key(|status| status.order);
    candidates
}

fn status_group_kind(status: &Status, status_groups: &[StatusGroup]) -> Option<StatusGroupKind> {
    status_groups
        .iter()
        .find(|group| group.id == status.group_id)
        .map(|group| group.kind.clone())
}

fn task_status_color(kind: &StatusGroupKind, palette: Palette) -> Color32 {
    match kind {
        StatusGroupKind::NotStarted => palette.muted,
        StatusGroupKind::InProgress => palette.accent,
        StatusGroupKind::Pending => palette.warning,
        StatusGroupKind::Done => palette.success,
    }
}

fn sorted_tasks(tasks: &[Task], mode: TaskSortMode) -> Vec<Task> {
    let mut tasks = tasks.to_vec();
    match mode {
        TaskSortMode::DueDate => {
            tasks.sort_by_key(|task| (task.due_date.is_none(), task.due_date, task.created_at));
        }
        TaskSortMode::Estimate => {
            tasks.sort_by_key(|task| {
                (
                    task.estimated_minutes.is_none(),
                    task.estimated_minutes.unwrap_or(u32::MAX),
                    task.due_date.is_none(),
                    task.due_date,
                    task.created_at,
                )
            });
        }
        TaskSortMode::Importance => {
            tasks.sort_by_key(|task| {
                (
                    Reverse(task.cost_points.unwrap_or(0)),
                    task.due_date.is_none(),
                    task.due_date,
                    task.created_at,
                )
            });
        }
        TaskSortMode::Created => {
            tasks.sort_by_key(|task| task.created_at);
        }
    }
    tasks
}

fn task_context_line(task: &Task, lists: &[List], projects: &[Project]) -> String {
    let list = task
        .list_id
        .as_ref()
        .and_then(|list_id| lists.iter().find(|list| &list.id == list_id))
        .map(|list| list.name.as_str());
    let project = task
        .project_id
        .as_ref()
        .and_then(|project_id| projects.iter().find(|project| &project.id == project_id))
        .map(|project| project.title.as_str());

    match (project, list) {
        (Some(project), Some(list)) => format!("{project} / {list}"),
        (Some(project), None) => project.to_string(),
        (None, Some(list)) => list.to_string(),
        (None, None) => "Inbox".to_string(),
    }
}

fn agenda_time_from_y_for_duration(
    y: f32,
    rect: egui::Rect,
    day_start: OffsetDateTime,
    total_minutes: f32,
    duration_minutes: i64,
) -> OffsetDateTime {
    let duration_minutes = duration_minutes.max(1) as f32;
    let max_start_minutes = (total_minutes - duration_minutes).max(0.0);
    let raw_minutes =
        ((y - rect.top()) / rect.height() * total_minutes).clamp(0.0, total_minutes - 1.0);
    let rounded_minutes =
        ((raw_minutes / 15.0).round() * 15.0).clamp(0.0, max_start_minutes) as i64;
    day_start + time::Duration::minutes(rounded_minutes)
}

fn agenda_pixels_for_minutes(minutes: i64, rect: egui::Rect, total_minutes: f32) -> f32 {
    minutes.max(0) as f32 * rect.height() / total_minutes
}

fn project_gantt_view(
    ui: &mut egui::Ui,
    projects: &[Project],
    selected_project_id: Option<&ProjectId>,
    selected_milestones: &[Milestone],
    palette: Palette,
) -> Option<ProjectId> {
    if projects.is_empty() {
        ui.label("No projects.");
        return None;
    }

    let Some((range_start, range_end)) = project_gantt_range(projects, selected_milestones) else {
        ui.label("No dated projects.");
        return None;
    };

    let total_days = (days_between(range_start, range_end) + 1).max(1) as f32;
    let timeline_width = (total_days * 24.0).clamp(560.0, 2200.0);
    let label_width = 220.0;
    let row_height = 44.0;
    let mut selected = None;

    ui.horizontal(|ui| {
        ui.label(regular_text(format!("{} - {}", range_start, range_end)).color(palette.muted));
        ui.label(regular_text(format!("{} projects", projects.len())).color(palette.muted));
    });
    ui.add_space(8.0);

    ScrollArea::both()
        .id_salt("project_gantt")
        .max_height(560.0)
        .show(ui, |ui| {
            draw_gantt_axis(
                ui,
                range_start,
                range_end,
                label_width,
                timeline_width,
                palette,
            );
            ui.add_space(4.0);

            for project in projects {
                ui.horizontal(|ui| {
                    let is_selected = selected_project_id == Some(&project.id);
                    if ui
                        .add_sized(
                            [label_width, 32.0],
                            egui::Button::selectable(is_selected, project.title.as_str()),
                        )
                        .clicked()
                    {
                        selected = Some(project.id.clone());
                    }

                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(timeline_width, row_height),
                        egui::Sense::click(),
                    );
                    draw_gantt_project_row(
                        ui.painter(),
                        rect,
                        project,
                        if is_selected {
                            selected_milestones
                        } else {
                            &[]
                        },
                        range_start,
                        range_end,
                        is_selected,
                        palette,
                    );
                    if response.clicked() {
                        selected = Some(project.id.clone());
                    }
                });
                ui.add_space(5.0);
            }
        });

    selected
}

fn draw_gantt_axis(
    ui: &mut egui::Ui,
    range_start: Date,
    range_end: Date,
    label_width: f32,
    timeline_width: f32,
    palette: Palette,
) {
    ui.horizontal(|ui| {
        ui.add_sized([label_width, 24.0], egui::Label::new(""));
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(timeline_width, 24.0), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        painter.line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            Stroke::new(1.0_f32, palette.border),
        );

        let mut cursor = range_start;
        let mut last_month = None;
        while cursor <= range_end {
            let is_month_start = last_month != Some((cursor.year(), cursor.month()));
            let is_week_start = cursor.weekday().number_days_from_monday() == 0;
            if is_month_start || is_week_start {
                let x = gantt_x_for_date(rect, range_start, range_end, cursor);
                let color = if is_month_start {
                    palette.accent
                } else {
                    palette.border
                };
                painter.line_segment(
                    [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                    Stroke::new(1.0_f32, color),
                );
                if is_month_start {
                    painter.text(
                        egui::pos2(x + 4.0, rect.top() + 2.0),
                        egui::Align2::LEFT_TOP,
                        format!("{} {}", month_label(cursor.month()), cursor.year()),
                        egui::FontId::proportional(12.0),
                        palette.muted,
                    );
                }
            }
            last_month = Some((cursor.year(), cursor.month()));
            let Some(next_day) = cursor.next_day() else {
                break;
            };
            cursor = next_day;
        }
    });
}

fn draw_gantt_project_row(
    painter: &egui::Painter,
    rect: egui::Rect,
    project: &Project,
    milestones: &[Milestone],
    range_start: Date,
    range_end: Date,
    selected: bool,
    palette: Palette,
) {
    painter.rect(
        rect,
        6.0,
        if selected {
            palette.selected_fill
        } else {
            palette.surface
        },
        Stroke::new(
            1.0_f32,
            if selected {
                palette.accent
            } else {
                palette.border
            },
        ),
        egui::StrokeKind::Inside,
    );

    draw_gantt_grid(painter, rect, range_start, range_end, palette);

    if let Some((start, end)) = project_gantt_span(project) {
        let start = start.max(range_start);
        let end = end.min(range_end);
        let left = gantt_x_for_date(rect, range_start, range_end, start);
        let right = gantt_x_for_date(rect, range_start, range_end, end.next_day().unwrap_or(end));
        let bar_rect = egui::Rect::from_min_max(
            egui::pos2(left, rect.center().y - 8.0),
            egui::pos2(right.max(left + 8.0), rect.center().y + 8.0),
        );
        painter.rect(
            bar_rect,
            8.0,
            if selected {
                palette.accent
            } else {
                palette.control_active
            },
            Stroke::new(1.0_f32, palette.border_strong),
            egui::StrokeKind::Inside,
        );
    } else {
        painter.text(
            rect.left_center() + egui::vec2(10.0, 0.0),
            egui::Align2::LEFT_CENTER,
            "No project dates",
            egui::FontId::proportional(12.0),
            palette.muted,
        );
    }

    for milestone in milestones {
        if milestone.target_date < range_start || milestone.target_date > range_end {
            continue;
        }
        let x = gantt_x_for_date(rect, range_start, range_end, milestone.target_date);
        let center = egui::pos2(x, rect.center().y);
        painter.circle_filled(
            center,
            5.0,
            milestone_status_color(&milestone.status, palette),
        );
        painter.text(
            center + egui::vec2(7.0, -16.0),
            egui::Align2::LEFT_TOP,
            truncate_chars(&milestone.title, 24),
            egui::FontId::proportional(12.0),
            palette.text,
        );
    }
}

fn draw_gantt_grid(
    painter: &egui::Painter,
    rect: egui::Rect,
    range_start: Date,
    range_end: Date,
    palette: Palette,
) {
    let mut cursor = range_start;
    while cursor <= range_end {
        if cursor.weekday().number_days_from_monday() == 0 {
            let x = gantt_x_for_date(rect, range_start, range_end, cursor);
            painter.line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                Stroke::new(1.0_f32, palette.faint),
            );
        }
        let Some(next_day) = cursor.next_day() else {
            break;
        };
        cursor = next_day;
    }

    let today = OffsetDateTime::now_utc().date();
    if today >= range_start && today <= range_end {
        let x = gantt_x_for_date(rect, range_start, range_end, today);
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            Stroke::new(1.5_f32, palette.warning),
        );
    }
}

fn project_gantt_range(
    projects: &[Project],
    selected_milestones: &[Milestone],
) -> Option<(Date, Date)> {
    let mut start = None::<Date>;
    let mut end = None::<Date>;

    for project in projects {
        if let Some((project_start, project_end)) = project_gantt_span(project) {
            start = Some(start.map_or(project_start, |value| value.min(project_start)));
            end = Some(end.map_or(project_end, |value| value.max(project_end)));
        }
    }

    for milestone in selected_milestones {
        start = Some(start.map_or(milestone.target_date, |value| {
            value.min(milestone.target_date)
        }));
        end = Some(end.map_or(milestone.target_date, |value| {
            value.max(milestone.target_date)
        }));
    }

    let start = start?;
    let end = end.unwrap_or(start);
    Some((
        add_days(start, -3).unwrap_or(start),
        add_days(end.max(start), 7).unwrap_or(end.max(start)),
    ))
}

fn project_gantt_span(project: &Project) -> Option<(Date, Date)> {
    let start = project.start_date.or(project.end_date)?;
    let end = project.end_date.or(project.start_date).unwrap_or(start);
    Some(if start <= end {
        (start, end)
    } else {
        (end, start)
    })
}

fn gantt_x_for_date(rect: egui::Rect, range_start: Date, range_end: Date, date: Date) -> f32 {
    let total_days = (days_between(range_start, range_end) + 1).max(1) as f32;
    let day_index = days_between(range_start, date).clamp(0, total_days as i64) as f32;
    rect.left() + rect.width() * (day_index / total_days)
}

fn calendar_grid_start(first_day: Date) -> Date {
    let mut date = first_day;
    for _ in 0..first_day.weekday().number_days_from_monday() {
        date = date.previous_day().unwrap_or(date);
    }
    date
}

fn first_day_of_month(date: Date) -> Result<Date> {
    Ok(Date::from_calendar_date(date.year(), date.month(), 1)?)
}

fn add_months(date: Date, months: i32) -> Result<Date> {
    let zero_based_month = date.year() * 12 + (date.month() as i32 - 1) + months;
    let year = zero_based_month.div_euclid(12);
    let month_index = zero_based_month.rem_euclid(12) + 1;
    let month = Month::try_from(month_index as u8)?;
    let day = date.day().min(days_in_month(year, month)?);
    Ok(Date::from_calendar_date(year, month, day)?)
}

fn days_in_month(year: i32, month: Month) -> Result<u8> {
    let month_number = month as u8;
    let (next_year, next_month) = if month_number == 12 {
        (year + 1, Month::January)
    } else {
        (year, Month::try_from(month_number + 1)?)
    };
    let first_next_month = Date::from_calendar_date(next_year, next_month, 1)?;
    Ok(first_next_month
        .previous_day()
        .ok_or_else(|| anyhow!("invalid month"))?
        .day())
}

fn add_days(date: Date, days: i32) -> Option<Date> {
    let mut result = date;
    for _ in 0..days.unsigned_abs() {
        result = if days < 0 {
            result.previous_day()?
        } else {
            result.next_day()?
        };
    }
    Some(result)
}

fn days_between(start: Date, end: Date) -> i64 {
    if start == end {
        return 0;
    }

    if start > end {
        return -days_between(end, start);
    }

    let mut current = start;
    let mut days = 0;
    while current < end {
        let Some(next_day) = current.next_day() else {
            break;
        };
        current = next_day;
        days += 1;
    }
    days
}

fn month_label(month: Month) -> &'static str {
    match month {
        Month::January => "January",
        Month::February => "February",
        Month::March => "March",
        Month::April => "April",
        Month::May => "May",
        Month::June => "June",
        Month::July => "July",
        Month::August => "August",
        Month::September => "September",
        Month::October => "October",
        Month::November => "November",
        Month::December => "December",
    }
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut truncated = value
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    truncated.push('…');
    truncated
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
    parse_optional_date_with_today(value, OffsetDateTime::now_utc().date())
}

fn parse_optional_date_with_today(value: &str, today: Date) -> Result<Option<Date>> {
    let value = value.trim();
    if value.is_empty() {
        Ok(None)
    } else {
        parse_flexible_date(value, today).map(Some)
    }
}

fn parse_required_date(value: &str) -> Result<Date> {
    Date::parse(value.trim(), format_description!("[year]-[month]-[day]"))
        .map_err(|_| anyhow!("日付は YYYY-MM-DD で入力してください"))
}

fn parse_flexible_date(value: &str, today: Date) -> Result<Date> {
    let value = value.trim();
    if let Ok(date) = parse_required_date(value) {
        return Ok(date);
    }

    let normalized = value.to_ascii_lowercase().replace(['/', '.'], "-");
    if let Ok(date) = Date::parse(&normalized, format_description!("[year]-[month]-[day]")) {
        return Ok(date);
    }

    if normalized.len() == 8 && normalized.chars().all(|ch| ch.is_ascii_digit()) {
        let year = normalized[0..4]
            .parse::<i32>()
            .map_err(|_| anyhow!("invalid year"))?;
        let month = normalized[4..6]
            .parse::<u8>()
            .map_err(|_| anyhow!("invalid month"))?;
        let day = normalized[6..8]
            .parse::<u8>()
            .map_err(|_| anyhow!("invalid day"))?;
        return Date::from_calendar_date(year, Month::try_from(month)?, day)
            .map_err(|_| anyhow!("日付は YYYY-MM-DD または YYYYMMDD で入力してください"));
    }

    match normalized.as_str() {
        "today" => return Ok(today),
        "tomorrow" | "tmr" | "tmrw" => {
            return today
                .next_day()
                .ok_or_else(|| anyhow!("日付を解釈できません"));
        }
        "yesterday" => {
            return today
                .previous_day()
                .ok_or_else(|| anyhow!("日付を解釈できません"));
        }
        _ => {}
    }

    if let Some(weekday) = parse_weekday(&normalized) {
        return Ok(next_weekday(today, weekday));
    }

    Err(anyhow!(
        "日付は YYYY-MM-DD / YYYYMMDD / today / tomorrow / tue などで入力してください"
    ))
}

#[cfg(test)]
fn parse_hm_for_date(value: &str, date: Date, timezone: UtcOffset) -> Result<OffsetDateTime> {
    let time = parse_hm_time(value)?;
    Ok(date.with_time(time).assume_offset(timezone))
}

fn parse_hm_time(value: &str) -> Result<Time> {
    Time::parse(value.trim(), format_description!("[hour]:[minute]"))
        .map_err(|_| anyhow!("時刻は HH:MM で入力してください"))
}

fn validate_availability_window(start: &str, end: &str) -> Result<()> {
    let start = parse_hm_time(start)?;
    let end = parse_hm_time(end)?;
    if start >= end {
        return Err(anyhow!(
            "Planning hours は開始時刻より後の終了時刻を指定してください"
        ));
    }
    Ok(())
}

fn parse_weekday(value: &str) -> Option<Weekday> {
    weekday_options()
        .into_iter()
        .find(|(label, _)| *label == value)
        .map(|(_, weekday)| weekday)
}

fn weekday_options() -> [(&'static str, Weekday); 14] {
    [
        ("mon", Weekday::Monday),
        ("monday", Weekday::Monday),
        ("tue", Weekday::Tuesday),
        ("tuesday", Weekday::Tuesday),
        ("wed", Weekday::Wednesday),
        ("wednesday", Weekday::Wednesday),
        ("thu", Weekday::Thursday),
        ("thursday", Weekday::Thursday),
        ("fri", Weekday::Friday),
        ("friday", Weekday::Friday),
        ("sat", Weekday::Saturday),
        ("saturday", Weekday::Saturday),
        ("sun", Weekday::Sunday),
        ("sunday", Weekday::Sunday),
    ]
}

fn next_weekday(today: Date, weekday: Weekday) -> Date {
    let today_index = today.weekday().number_days_from_monday() as i32;
    let target_index = weekday.number_days_from_monday() as i32;
    let offset = (target_index - today_index).rem_euclid(7);
    add_days(today, offset).unwrap_or(today)
}

fn parse_duration_minutes(value: &str) -> Option<u32> {
    let normalized = value.trim().to_ascii_lowercase().replace(' ', "");
    if normalized.is_empty() {
        return None;
    }
    if let Ok(minutes) = normalized.parse::<u32>() {
        return Some(minutes);
    }
    if let Some(minutes) = parse_colon_duration(&normalized) {
        return Some(minutes);
    }
    parse_compact_duration(&normalized).or_else(|| parse_duration_token(&normalized))
}

fn parse_colon_duration(value: &str) -> Option<u32> {
    let parts = value.split(':').collect::<Vec<_>>();
    match parts.as_slice() {
        [hours, minutes] => Some(hours.parse::<u32>().ok()? * 60 + minutes.parse::<u32>().ok()?),
        [hours, minutes, seconds] => {
            let seconds = seconds.parse::<u32>().ok()?;
            Some(
                hours.parse::<u32>().ok()? * 60
                    + minutes.parse::<u32>().ok()?
                    + u32::from(seconds > 0),
            )
        }
        _ => None,
    }
}

fn parse_compact_duration(value: &str) -> Option<u32> {
    let mut total = 0_u32;
    let mut digits = String::new();
    let mut saw_unit = false;
    for ch in value.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
            continue;
        }
        if digits.is_empty() {
            return None;
        }
        let amount = digits.parse::<u32>().ok()?;
        digits.clear();
        match ch {
            'h' => {
                total = total.saturating_add(amount.saturating_mul(60));
                saw_unit = true;
            }
            'm' => {
                total = total.saturating_add(amount);
                saw_unit = true;
            }
            _ => return None,
        }
    }
    if !digits.is_empty() || !saw_unit {
        return None;
    }
    Some(total.max(1))
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
            due_date = Some(parse_flexible_date(raw_due, today)?);
            index += 2;
            continue;
        }

        if let Some(raw_due) = normalized
            .strip_prefix("/due:")
            .or_else(|| normalized.strip_prefix("due:"))
        {
            due_date = Some(parse_flexible_date(raw_due, today)?);
            index += 1;
            continue;
        }

        if matches!(normalized.as_str(), "/m" | "/minutes" | "minutes") {
            let Some(raw_minutes) = tokens.get(index + 1) else {
                return Err(anyhow!("見積分数を入力してください"));
            };
            estimated_minutes = Some(parse_duration_minutes(raw_minutes).ok_or_else(|| {
                anyhow!("見積時間は 30m / 1h30m / 01:30 / 分数 などで入力してください")
            })?);
            index += 2;
            continue;
        }

        if let Some(minutes) = parse_duration_minutes(&normalized) {
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
        status_id: None,
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

#[cfg(test)]
fn workday_availability(
    date: Date,
    timezone: UtcOffset,
    start_time: &str,
    end_time: &str,
) -> Result<Vec<mnema_app::AvailabilityWindow>> {
    validate_availability_window(start_time, end_time)?;
    let now = OffsetDateTime::now_utc().to_offset(timezone);
    let today = now.date();
    if date < today {
        return Ok(Vec::new());
    }

    let mut start = parse_hm_for_date(start_time, date, timezone)?;
    let end = parse_hm_for_date(end_time, date, timezone)?;
    if date == today {
        start = start.max(now);
    }
    if start >= end {
        return Ok(Vec::new());
    }

    Ok(vec![mnema_app::AvailabilityWindow {
        window: mnema_app::TimeWindow::new(start, end),
    }])
}

fn format_hm(value: OffsetDateTime) -> String {
    value
        .format(format_description!("[hour]:[minute]"))
        .unwrap_or_else(|_| value.time().to_string())
}

fn format_hm_in(value: OffsetDateTime, timezone: UtcOffset) -> String {
    format_hm(value.to_offset(timezone))
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

fn schedule_issue_label(issue: &ScheduleIssue) -> String {
    match issue {
        ScheduleIssue::NoAvailability => {
            "No availability in the remaining planning window".to_string()
        }
        ScheduleIssue::TaskUnscheduled {
            title,
            required_minutes,
            ..
        } => format!("Could not schedule {title} ({required_minutes}m required)"),
    }
}

fn habit_schedule_label(habit: &Habit) -> String {
    let schedule = match &habit.schedule {
        HabitSchedule::Daily => "Daily".to_string(),
        HabitSchedule::Weekdays { weekdays } => weekdays
            .iter()
            .map(day_of_week_label)
            .collect::<Vec<_>>()
            .join(", "),
    };
    let window = habit.preferred_window.map_or_else(
        || "any time".to_string(),
        |range| format!("{}–{}", format_time(range.start), format_time(range.end)),
    );
    format!("{schedule} · {window}")
}

fn day_of_week_label(day: &DayOfWeek) -> &'static str {
    match day {
        DayOfWeek::Monday => "Mon",
        DayOfWeek::Tuesday => "Tue",
        DayOfWeek::Wednesday => "Wed",
        DayOfWeek::Thursday => "Thu",
        DayOfWeek::Friday => "Fri",
        DayOfWeek::Saturday => "Sat",
        DayOfWeek::Sunday => "Sun",
    }
}

fn calendar_sync_label(cursor: Option<&CalendarSyncCursor>) -> String {
    let Some(cursor) = cursor else {
        return "not synced".into();
    };
    cursor
        .last_incremental_sync_at
        .or(cursor.last_full_sync_at)
        .map(|value| format!("synced {value}"))
        .unwrap_or_else(|| "sync initialized".into())
}

fn format_time(value: Time) -> String {
    value
        .format(format_description!("[hour]:[minute]"))
        .unwrap_or_else(|_| value.to_string())
}

fn normalized_iana_timezone(value: &str) -> String {
    let value = value.trim();
    if timezones::get_by_name(value).is_some() {
        return value.to_owned();
    }
    if value.eq_ignore_ascii_case("jst") || value == "+09:00" || value.is_empty() {
        return "Asia/Tokyo".into();
    }
    if value.eq_ignore_ascii_case("utc") || value == "+00:00" || value == "Z" {
        return "UTC".into();
    }
    "UTC".into()
}

fn milestone_status_label(status: &MilestoneStatus) -> &'static str {
    match status {
        MilestoneStatus::NotDone => "not done",
        MilestoneStatus::Overdue => "overdue",
        MilestoneStatus::Done => "done",
    }
}

fn milestone_status_color(status: &MilestoneStatus, palette: Palette) -> Color32 {
    match status {
        MilestoneStatus::NotDone => palette.accent,
        MilestoneStatus::Overdue => palette.warning,
        MilestoneStatus::Done => palette.success,
    }
}

fn automation_action_label(action_type: &AutomationActionType) -> String {
    match action_type {
        AutomationActionType::Move => "タスクの所属を変更".to_string(),
        AutomationActionType::UpdateDue => "期限を変更".to_string(),
        AutomationActionType::Classify => "タスクを分類".to_string(),
        AutomationActionType::CreateTask => "タスクを作成".to_string(),
        AutomationActionType::UpdateStatus => "ステータスを変更".to_string(),
        AutomationActionType::Other(value) => value.clone(),
    }
}

fn automation_log_title(log: &AutomationLog) -> Option<String> {
    log.after_state
        .as_ref()
        .and_then(|value| value.get("title"))
        .and_then(|value| value.as_str())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    fn test_task(
        title: &str,
        due_date: Option<Date>,
        estimated_minutes: Option<u32>,
        cost_points: Option<u32>,
        created_hour: u8,
    ) -> Task {
        Task {
            id: TaskId::new(),
            title: title.to_string(),
            description: None,
            project_id: None,
            list_id: None,
            status_id: StatusId::new(),
            due_date,
            start_date: None,
            estimated_minutes,
            cost_points,
            dependencies: Vec::new(),
            milestone_id: None,
            created_at: date!(2026 - 06 - 25)
                .with_hms(created_hour, 0, 0)
                .unwrap()
                .assume_utc(),
            updated_at: date!(2026 - 06 - 25)
                .with_hms(created_hour, 0, 0)
                .unwrap()
                .assume_utc(),
            deleted_at: None,
        }
    }

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

    #[test]
    fn sorts_home_tasks_by_due_estimate_and_importance() {
        let tasks = vec![
            test_task("no due", None, Some(60), Some(1), 9),
            test_task("soon", Some(date!(2026 - 06 - 26)), Some(120), Some(3), 10),
            test_task("short", Some(date!(2026 - 06 - 27)), Some(15), Some(2), 11),
        ];

        assert_eq!(
            sorted_tasks(&tasks, TaskSortMode::DueDate)
                .iter()
                .map(|task| task.title.as_str())
                .collect::<Vec<_>>(),
            ["soon", "short", "no due"]
        );
        assert_eq!(
            sorted_tasks(&tasks, TaskSortMode::Estimate)
                .iter()
                .map(|task| task.title.as_str())
                .collect::<Vec<_>>(),
            ["short", "no due", "soon"]
        );
        assert_eq!(
            sorted_tasks(&tasks, TaskSortMode::Importance)
                .iter()
                .map(|task| task.title.as_str())
                .collect::<Vec<_>>(),
            ["soon", "short", "no due"]
        );
    }

    #[test]
    fn parses_flexible_dates() {
        let today = date!(2026 - 06 - 25);

        assert_eq!(
            parse_flexible_date("20260630", today).unwrap(),
            date!(2026 - 06 - 30)
        );
        assert_eq!(
            parse_flexible_date("2026/07/01", today).unwrap(),
            date!(2026 - 07 - 01)
        );
        assert_eq!(
            parse_flexible_date("tomorrow", today).unwrap(),
            date!(2026 - 06 - 26)
        );
        assert_eq!(
            parse_flexible_date("tue", today).unwrap(),
            date!(2026 - 06 - 30)
        );
    }

    #[test]
    fn parses_duration_inputs() {
        assert_eq!(parse_duration_minutes("30m"), Some(30));
        assert_eq!(parse_duration_minutes("1h30m"), Some(90));
        assert_eq!(parse_duration_minutes("258m"), Some(258));
        assert_eq!(parse_duration_minutes("01:30"), Some(90));
        assert_eq!(parse_duration_minutes("01:30:01"), Some(91));
    }

    #[test]
    fn parses_timezone_offsets() {
        assert_eq!(
            parse_timezone_offset("JST").unwrap(),
            UtcOffset::from_hms(9, 0, 0).unwrap()
        );
        assert_eq!(
            parse_timezone_offset("+09:00").unwrap(),
            UtcOffset::from_hms(9, 0, 0).unwrap()
        );
        assert_eq!(
            parse_timezone_offset("-05:30").unwrap(),
            UtcOffset::from_hms(-5, -30, 0).unwrap()
        );
        assert_eq!(parse_timezone_offset("UTC").unwrap(), UtcOffset::UTC);
    }

    #[test]
    fn builds_workday_availability_from_settings() {
        let timezone = UtcOffset::from_hms(9, 0, 0).unwrap();
        let target_date = OffsetDateTime::now_utc()
            .to_offset(timezone)
            .date()
            .next_day()
            .unwrap();

        let availability = workday_availability(target_date, timezone, "10:30", "15:00").unwrap();

        assert_eq!(availability.len(), 1);
        assert_eq!(
            format_hm_in(availability[0].window.start, timezone),
            "10:30"
        );
        assert_eq!(format_hm_in(availability[0].window.end, timezone), "15:00");
        assert!(validate_availability_window("17:00", "09:00").is_err());
    }

    #[test]
    fn rounds_agenda_drop_to_quarter_hour() {
        let timezone = UtcOffset::from_hms(9, 0, 0).unwrap();
        let day_start = date!(2026 - 06 - 25)
            .with_hms(0, 0, 0)
            .unwrap()
            .assume_offset(timezone);
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 1440.0));

        assert_eq!(
            format_hm(agenda_time_from_y_for_duration(
                548.0, rect, day_start, 1440.0, 15
            )),
            "09:15"
        );
        assert_eq!(
            format_hm(agenda_time_from_y_for_duration(
                1438.0, rect, day_start, 1440.0, 90
            )),
            "22:30"
        );
    }
}
