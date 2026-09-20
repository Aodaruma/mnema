use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
    env, fs,
    sync::Arc,
};

use anyhow::Context;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use mnema_app::{
    AvailabilityWindow, BusyBlock, BusyBlockSource, CaptureTaskRequest, CaptureTaskService,
    PlanTodayRequest, PlanTodayResult, PlanTodayService, ScheduleIssue, SchedulePlanStoreService,
    TimeWindow,
};
use mnema_core::prelude::*;
use mnema_infra::calendar::{
    CredentialStore, EncryptedFileCredentialStore, KeyringCredentialStore,
};
use mnema_infra::db::{StorageBackend, Vault};
use serde::{Deserialize, Serialize};
use time::{Date, OffsetDateTime, UtcOffset, format_description::well_known::Rfc3339};
use uuid::Uuid;

use crate::calendar_api::CalendarRuntime;
use crate::config::{ServerConfig, format_date, parse_clock, parse_date, parse_utc_offset};

#[derive(Clone)]
pub struct AppState {
    pub vault: Vault,
    pub config: Arc<ServerConfig>,
    pub calendar: Arc<CalendarRuntime>,
}

impl AppState {
    pub fn new(vault: Vault, config: Arc<ServerConfig>) -> Self {
        Self::with_credentials(vault, config, Arc::new(KeyringCredentialStore::mnema()))
    }

    pub fn from_env(vault: Vault, config: Arc<ServerConfig>) -> anyhow::Result<Self> {
        let credentials: Arc<dyn CredentialStore> =
            if let Ok(encoded_key) = env::var("MNEMA_CREDENTIAL_KEY") {
                Arc::new(EncryptedFileCredentialStore::from_base64_key(
                    vault.root.join(".credentials"),
                    encoded_key.trim(),
                )?)
            } else if let Ok(key_file) = env::var("MNEMA_CREDENTIAL_KEY_FILE") {
                let encoded_key = fs::read_to_string(&key_file)
                    .with_context(|| format!("failed to read credential key file: {key_file}"))?;
                Arc::new(EncryptedFileCredentialStore::from_base64_key(
                    vault.root.join(".credentials"),
                    encoded_key.trim(),
                )?)
            } else {
                Arc::new(KeyringCredentialStore::mnema())
            };
        Ok(Self::with_credentials(vault, config, credentials))
    }

    pub fn with_credentials(
        vault: Vault,
        config: Arc<ServerConfig>,
        credentials: Arc<dyn CredentialStore>,
    ) -> Self {
        Self {
            vault,
            config,
            calendar: Arc::new(CalendarRuntime::from_env(credentials)),
        }
    }
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .route("/tasks", get(list_tasks).post(create_task))
        .route(
            "/tasks/{id}",
            get(get_task).put(update_task).delete(delete_task),
        )
        .route("/plans/today/preview", post(preview_today))
        .route("/plans/today/apply", post(apply_today))
        .route("/schedule", get(get_schedule))
        .merge(crate::scheduling_api::router())
        .merge(crate::calendar_api::router())
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: ErrorDetail,
}

#[derive(Debug, Serialize)]
struct ErrorDetail {
    code: &'static str,
    message: String,
}

#[derive(Debug)]
pub(crate) struct ApiError {
    pub(crate) status: StatusCode,
    pub(crate) code: &'static str,
    pub(crate) message: String,
}

impl ApiError {
    pub(crate) fn bad_request(message: impl std::fmt::Display) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "bad_request",
            message: message.to_string(),
        }
    }

    pub(crate) fn not_found(message: impl std::fmt::Display) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: "not_found",
            message: message.to_string(),
        }
    }

    pub(crate) fn conflict(message: impl std::fmt::Display) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            code: "conflict",
            message: message.to_string(),
        }
    }

    pub(crate) fn unauthorized(message: impl std::fmt::Display) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "authorization_required",
            message: message.to_string(),
        }
    }

    pub(crate) fn forbidden(message: impl std::fmt::Display) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            code: "forbidden",
            message: message.to_string(),
        }
    }

    pub(crate) fn service_unavailable(message: impl std::fmt::Display) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "calendar_not_configured",
            message: message.to_string(),
        }
    }

    pub(crate) fn bad_gateway(error: impl std::fmt::Display) -> Self {
        tracing::warn!(error = %error, "calendar provider request failed");
        Self {
            status: StatusCode::BAD_GATEWAY,
            code: "calendar_provider_error",
            message: error.to_string(),
        }
    }

    pub(crate) fn internal(error: impl std::fmt::Display) -> Self {
        tracing::error!(error = %error, "request failed");
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "internal_error",
            message: "The server could not complete the request.".to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorBody {
                error: ErrorDetail {
                    code: self.code,
                    message: self.message,
                },
            }),
        )
            .into_response()
    }
}

pub(crate) type ApiResult<T> = Result<T, ApiError>;

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
    backend: &'static str,
    server_time: String,
    defaults: ClientDefaults,
    integrations: IntegrationHealth,
}

#[derive(Debug, Serialize)]
struct ClientDefaults {
    timezone: String,
    timezone_offset: String,
    planning_start: String,
    planning_end: String,
    refresh_seconds: u64,
    automation_mode: &'static str,
    automation_interval_seconds: u64,
}

#[derive(Debug, Serialize)]
struct IntegrationHealth {
    google_calendar_configured: bool,
}

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    let backend = match state.vault.backend() {
        StorageBackend::Sqlite => "sqlite",
        StorageBackend::Postgres => "postgres",
    };
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        backend,
        server_time: format_timestamp(OffsetDateTime::now_utc()),
        defaults: ClientDefaults {
            timezone: state.config.timezone.clone(),
            timezone_offset: state.config.timezone_offset.clone(),
            planning_start: state.config.planning_start.clone(),
            planning_end: state.config.planning_end.clone(),
            refresh_seconds: state.config.refresh_seconds,
            automation_mode: state.config.automation_mode.as_str(),
            automation_interval_seconds: state.config.automation_interval_seconds,
        },
        integrations: IntegrationHealth {
            google_calendar_configured: state.calendar.is_configured(),
        },
    })
}

#[derive(Debug, Clone, Serialize)]
struct TaskDto {
    id: String,
    title: String,
    description: Option<String>,
    due_date: Option<String>,
    start_date: Option<String>,
    estimated_minutes: Option<u32>,
    completed: bool,
    status_id: String,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize)]
struct TaskListResponse {
    tasks: Vec<TaskDto>,
}

#[derive(Debug, Deserialize)]
struct CreateTaskInput {
    title: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    due_date: Option<String>,
    #[serde(default)]
    estimated_minutes: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct UpdateTaskInput {
    title: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    due_date: Option<String>,
    #[serde(default)]
    estimated_minutes: Option<u32>,
    #[serde(default)]
    completed: Option<bool>,
}

async fn list_tasks(State(state): State<AppState>) -> ApiResult<Json<TaskListResponse>> {
    let repo = state.vault.task_repo();
    let mut tasks = repo.list_all().await.map_err(ApiError::internal)?;
    tasks.retain(|task| task.deleted_at.is_none());
    tasks.sort_by_key(|task| Reverse(task.created_at));
    let done_statuses = done_status_ids(&state.vault, &tasks).await?;
    Ok(Json(TaskListResponse {
        tasks: tasks
            .into_iter()
            .map(|task| task_dto(task, &done_statuses))
            .collect(),
    }))
}

async fn get_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<TaskDto>> {
    let task_id = parse_task_id(&id)?;
    let repo = state.vault.task_repo();
    let task = repo
        .find(task_id)
        .await
        .map_err(ApiError::internal)?
        .filter(|task| task.deleted_at.is_none())
        .ok_or_else(|| ApiError::not_found("Task not found."))?;
    let done_statuses = done_status_ids(&state.vault, std::slice::from_ref(&task)).await?;
    Ok(Json(task_dto(task, &done_statuses)))
}

async fn create_task(
    State(state): State<AppState>,
    Json(input): Json<CreateTaskInput>,
) -> ApiResult<(StatusCode, Json<TaskDto>)> {
    validate_task_input(&input.title, input.estimated_minutes)?;
    let task_repo = state.vault.task_repo();
    let list_repo = state.vault.list_repo();
    let status_repo = state.vault.status_repo();
    let service =
        CaptureTaskService::new(task_repo.as_ref(), list_repo.as_ref(), status_repo.as_ref());
    let result = service
        .capture_inbox_task(CaptureTaskRequest {
            title: input.title,
            description: normalize_description(input.description),
            due_date: parse_optional_date(input.due_date)?,
            estimated_minutes: input.estimated_minutes,
        })
        .await
        .map_err(ApiError::internal)?;
    let done_statuses = done_status_ids(&state.vault, std::slice::from_ref(&result.task)).await?;
    Ok((
        StatusCode::CREATED,
        Json(task_dto(result.task, &done_statuses)),
    ))
}

async fn update_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<UpdateTaskInput>,
) -> ApiResult<Json<TaskDto>> {
    validate_task_input(&input.title, input.estimated_minutes)?;
    let task_id = parse_task_id(&id)?;
    let repo = state.vault.task_repo();
    let mut task = repo
        .find(task_id)
        .await
        .map_err(ApiError::internal)?
        .filter(|task| task.deleted_at.is_none())
        .ok_or_else(|| ApiError::not_found("Task not found."))?;

    task.title = input.title.trim().to_string();
    task.description = normalize_description(input.description);
    task.due_date = parse_optional_date(input.due_date)?;
    task.estimated_minutes = input.estimated_minutes;
    if let Some(completed) = input.completed {
        task.status_id =
            status_id_for_completion(&state.vault, task.project_id.clone(), completed).await?;
    }
    task.updated_at = OffsetDateTime::now_utc();
    repo.update(task.clone())
        .await
        .map_err(ApiError::internal)?;

    let done_statuses = done_status_ids(&state.vault, std::slice::from_ref(&task)).await?;
    Ok(Json(task_dto(task, &done_statuses)))
}

async fn delete_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let task_id = parse_task_id(&id)?;
    let repo = state.vault.task_repo();
    if repo
        .find(task_id.clone())
        .await
        .map_err(ApiError::internal)?
        .filter(|task| task.deleted_at.is_none())
        .is_none()
    {
        return Err(ApiError::not_found("Task not found."));
    }
    repo.soft_delete(task_id, OffsetDateTime::now_utc())
        .await
        .map_err(ApiError::internal)?;
    Ok(StatusCode::NO_CONTENT)
}

fn validate_task_input(title: &str, estimate: Option<u32>) -> ApiResult<()> {
    if title.trim().is_empty() {
        return Err(ApiError::bad_request("Task title is required."));
    }
    if matches!(estimate, Some(0 | 10081..)) {
        return Err(ApiError::bad_request(
            "Estimate must be between 1 and 10080 minutes.",
        ));
    }
    Ok(())
}

fn normalize_description(value: Option<String>) -> Option<String> {
    value.and_then(|description| {
        let description = description.trim().to_string();
        (!description.is_empty()).then_some(description)
    })
}

fn parse_optional_date(value: Option<String>) -> ApiResult<Option<Date>> {
    value
        .filter(|value| !value.trim().is_empty())
        .map(|value| parse_date(&value).map_err(ApiError::bad_request))
        .transpose()
}

fn parse_task_id(value: &str) -> ApiResult<TaskId> {
    Uuid::parse_str(value)
        .map(TaskId::from)
        .map_err(|_| ApiError::bad_request("Task id must be a UUID."))
}

fn task_dto(task: Task, done_statuses: &HashSet<StatusId>) -> TaskDto {
    TaskDto {
        id: task.id.0.to_string(),
        title: task.title,
        description: task.description,
        due_date: task.due_date.map(format_date),
        start_date: task.start_date.map(format_date),
        estimated_minutes: task.estimated_minutes,
        completed: done_statuses.contains(&task.status_id),
        status_id: task.status_id.0.to_string(),
        created_at: format_timestamp(task.created_at),
        updated_at: format_timestamp(task.updated_at),
    }
}

async fn done_status_ids(vault: &Vault, tasks: &[Task]) -> ApiResult<HashSet<StatusId>> {
    let status_repo = vault.status_repo();
    let groups = status_repo
        .list_groups()
        .await
        .map_err(ApiError::internal)?;
    let done_groups = groups
        .into_iter()
        .filter(|group| matches!(group.kind, StatusGroupKind::Done))
        .map(|group| group.id)
        .collect::<HashSet<_>>();
    let mut scopes = HashSet::from([None]);
    scopes.extend(tasks.iter().map(|task| task.project_id.clone()));
    let mut result = HashSet::new();
    for scope in scopes {
        let statuses = status_repo
            .list_statuses_for_project(scope)
            .await
            .map_err(ApiError::internal)?;
        result.extend(
            statuses
                .into_iter()
                .filter(|status| done_groups.contains(&status.group_id))
                .map(|status| status.id),
        );
    }
    Ok(result)
}

async fn status_id_for_completion(
    vault: &Vault,
    project_id: Option<ProjectId>,
    completed: bool,
) -> ApiResult<StatusId> {
    let status_repo = vault.status_repo();
    let groups = status_repo
        .list_groups()
        .await
        .map_err(ApiError::internal)?;
    let group_kinds = groups
        .into_iter()
        .map(|group| (group.id, group.kind))
        .collect::<HashMap<_, _>>();
    let desired = if completed {
        StatusGroupKind::Done
    } else {
        StatusGroupKind::NotStarted
    };
    let scopes = if let Some(project_id) = project_id {
        vec![Some(project_id), None]
    } else {
        vec![None]
    };
    for scope in scopes {
        let statuses = status_repo
            .list_statuses_for_project(scope)
            .await
            .map_err(ApiError::internal)?;
        if let Some(status) = statuses.into_iter().find(|status| {
            group_kinds
                .get(&status.group_id)
                .is_some_and(|kind| kind == &desired)
        }) {
            return Ok(status.id);
        }
    }
    Err(ApiError::internal("No matching task status is configured."))
}

#[derive(Debug, Clone, Deserialize, Default)]
struct PlanInput {
    #[serde(default)]
    date: Option<String>,
    #[serde(default)]
    availability_start: Option<String>,
    #[serde(default)]
    availability_end: Option<String>,
    #[serde(default)]
    timezone_offset: Option<String>,
}

#[derive(Debug)]
struct ResolvedPlanInput {
    date: Date,
    timezone: UtcOffset,
    availability_start: String,
    availability_end: String,
}

#[derive(Debug, Serialize)]
struct PlanResponse {
    date: String,
    applied: bool,
    blocks: Vec<PlanBlockDto>,
    unscheduled: Vec<String>,
    issues: Vec<PlanIssueDto>,
    schedule: Vec<ScheduleBlockDto>,
}

#[derive(Debug, Serialize)]
struct PlanBlockDto {
    task_id: String,
    title: String,
    start_at: String,
    end_at: String,
    required_minutes: u32,
}

#[derive(Debug, Serialize)]
struct PlanIssueDto {
    code: &'static str,
    message: String,
    task_id: Option<String>,
    required_minutes: Option<u32>,
}

async fn preview_today(
    State(state): State<AppState>,
    Json(input): Json<PlanInput>,
) -> ApiResult<Json<PlanResponse>> {
    Ok(Json(run_plan(&state, input, false).await?))
}

async fn apply_today(
    State(state): State<AppState>,
    Json(input): Json<PlanInput>,
) -> ApiResult<Json<PlanResponse>> {
    Ok(Json(run_plan(&state, input, true).await?))
}

async fn run_plan(state: &AppState, input: PlanInput, apply: bool) -> ApiResult<PlanResponse> {
    let input = resolve_plan_input(state, input)?;
    let availability = availability_for(&input)?;
    let schedule_repo = state.vault.schedule_block_repo();
    let current_schedule = schedule_repo
        .list_for_day(input.date)
        .await
        .map_err(ApiError::internal)?;
    let busy_blocks = current_schedule
        .iter()
        .filter(|block| is_fixed_block(block))
        .filter_map(schedule_to_busy_block)
        .collect::<Vec<_>>();

    let task_repo = state.vault.task_repo();
    let status_repo = state.vault.status_repo();
    let service = PlanTodayService::new(task_repo.as_ref(), status_repo.as_ref());
    let plan = service
        .plan_today(PlanTodayRequest {
            target_date: input.date,
            availability,
            busy_blocks,
        })
        .await
        .map_err(ApiError::internal)?;

    if apply {
        SchedulePlanStoreService::new(schedule_repo.as_ref())
            .save_proposed_plan(&plan)
            .await
            .map_err(ApiError::internal)?;
    }

    let schedule = if apply {
        schedule_repo
            .list_for_day(input.date)
            .await
            .map_err(ApiError::internal)?
    } else {
        current_schedule
    };
    Ok(plan_response(plan, apply, schedule))
}

fn resolve_plan_input(state: &AppState, input: PlanInput) -> ApiResult<ResolvedPlanInput> {
    let timezone_text = input
        .timezone_offset
        .unwrap_or_else(|| state.config.timezone_offset.clone());
    let timezone = parse_utc_offset(&timezone_text).map_err(ApiError::bad_request)?;
    let date = input
        .date
        .filter(|value| !value.trim().is_empty())
        .map(|value| parse_date(&value).map_err(ApiError::bad_request))
        .transpose()?
        .unwrap_or_else(|| OffsetDateTime::now_utc().to_offset(timezone).date());
    Ok(ResolvedPlanInput {
        date,
        timezone,
        availability_start: input
            .availability_start
            .unwrap_or_else(|| state.config.planning_start.clone()),
        availability_end: input
            .availability_end
            .unwrap_or_else(|| state.config.planning_end.clone()),
    })
}

fn availability_for(input: &ResolvedPlanInput) -> ApiResult<Vec<AvailabilityWindow>> {
    let start_time = parse_clock(&input.availability_start).map_err(ApiError::bad_request)?;
    let end_time = parse_clock(&input.availability_end).map_err(ApiError::bad_request)?;
    if start_time >= end_time {
        return Err(ApiError::bad_request(
            "Planning end must be after planning start.",
        ));
    }
    let now = OffsetDateTime::now_utc().to_offset(input.timezone);
    if input.date < now.date() {
        return Ok(Vec::new());
    }
    let mut start = input
        .date
        .with_time(start_time)
        .assume_offset(input.timezone);
    let end = input.date.with_time(end_time).assume_offset(input.timezone);
    if input.date == now.date() {
        start = start.max(now);
    }
    if start >= end {
        return Ok(Vec::new());
    }
    Ok(vec![AvailabilityWindow {
        window: TimeWindow::new(start, end),
    }])
}

fn is_fixed_block(block: &ScheduleBlock) -> bool {
    block.locked
        || matches!(&block.source, ScheduleBlockSource::ExternalCalendar)
        || matches!(
            &block.state,
            ScheduleBlockState::Scheduled | ScheduleBlockState::Active | ScheduleBlockState::Done
        )
}

fn schedule_to_busy_block(block: &ScheduleBlock) -> Option<BusyBlock> {
    if block.end_at <= block.start_at {
        return None;
    }
    let source = if matches!(&block.source, ScheduleBlockSource::ExternalCalendar) {
        BusyBlockSource::ExternalCalendar
    } else if block.locked {
        BusyBlockSource::LockedSchedule
    } else {
        BusyBlockSource::Manual
    };
    Some(BusyBlock {
        window: TimeWindow::new(block.start_at, block.end_at),
        source,
        label: block.title_snapshot.clone(),
    })
}

fn plan_response(
    plan: PlanTodayResult,
    applied: bool,
    schedule: Vec<ScheduleBlock>,
) -> PlanResponse {
    let blocks = plan
        .output
        .blocks
        .into_iter()
        .map(|block| PlanBlockDto {
            task_id: block.task_id.0.to_string(),
            title: block.title,
            start_at: format_timestamp(block.window.start),
            end_at: format_timestamp(block.window.end),
            required_minutes: block.required_minutes,
        })
        .collect();
    let issues = plan
        .output
        .issues
        .into_iter()
        .map(|issue| match issue {
            ScheduleIssue::NoAvailability => PlanIssueDto {
                code: "no_availability",
                message: "No planning time remains in the selected window.".to_string(),
                task_id: None,
                required_minutes: None,
            },
            ScheduleIssue::TaskUnscheduled {
                task_id,
                title,
                required_minutes,
            } => PlanIssueDto {
                code: "task_unscheduled",
                message: format!("{title} does not fit in the remaining time."),
                task_id: Some(task_id.0.to_string()),
                required_minutes: Some(required_minutes),
            },
        })
        .collect();
    PlanResponse {
        date: format_date(plan.target_date),
        applied,
        blocks,
        unscheduled: plan
            .output
            .unscheduled
            .into_iter()
            .map(|id| id.0.to_string())
            .collect(),
        issues,
        schedule: schedule.into_iter().map(schedule_block_dto).collect(),
    }
}

#[derive(Debug, Deserialize)]
struct ScheduleQuery {
    #[serde(default)]
    date: Option<String>,
}

#[derive(Debug, Serialize)]
struct ScheduleResponse {
    date: String,
    blocks: Vec<ScheduleBlockDto>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ScheduleBlockDto {
    pub id: String,
    pub task_id: Option<String>,
    pub habit_occurrence_id: Option<String>,
    pub title: String,
    pub start_at: String,
    pub end_at: String,
    pub block_type: &'static str,
    pub state: &'static str,
    pub source: &'static str,
    pub locked: bool,
}

async fn get_schedule(
    State(state): State<AppState>,
    Query(query): Query<ScheduleQuery>,
) -> ApiResult<Json<ScheduleResponse>> {
    let timezone = parse_utc_offset(&state.config.timezone_offset).map_err(ApiError::internal)?;
    let date = query
        .date
        .filter(|value| !value.trim().is_empty())
        .map(|value| parse_date(&value).map_err(ApiError::bad_request))
        .transpose()?
        .unwrap_or_else(|| OffsetDateTime::now_utc().to_offset(timezone).date());
    let blocks = state
        .vault
        .schedule_block_repo()
        .list_for_day(date)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(ScheduleResponse {
        date: format_date(date),
        blocks: blocks.into_iter().map(schedule_block_dto).collect(),
    }))
}

pub(crate) fn schedule_block_dto(block: ScheduleBlock) -> ScheduleBlockDto {
    ScheduleBlockDto {
        id: block.id.0.to_string(),
        task_id: block.task_id.map(|id| id.0.to_string()),
        habit_occurrence_id: block.habit_occurrence_id.map(|id| id.0.to_string()),
        title: block
            .title_snapshot
            .unwrap_or_else(|| "Untitled block".to_string()),
        start_at: format_timestamp(block.start_at),
        end_at: format_timestamp(block.end_at),
        block_type: match block.block_type {
            ScheduleBlockType::Task => "task",
            ScheduleBlockType::Habit => "habit",
            ScheduleBlockType::ExternalEvent => "external_event",
            ScheduleBlockType::Buffer => "buffer",
        },
        state: match block.state {
            ScheduleBlockState::Proposed => "proposed",
            ScheduleBlockState::Scheduled => "scheduled",
            ScheduleBlockState::Active => "active",
            ScheduleBlockState::Done => "done",
            ScheduleBlockState::Missed => "missed",
            ScheduleBlockState::Cancelled => "cancelled",
        },
        source: match block.source {
            ScheduleBlockSource::Scheduler => "scheduler",
            ScheduleBlockSource::Repair => "repair",
            ScheduleBlockSource::Manual => "manual",
            ScheduleBlockSource::ExternalCalendar => "external_calendar",
        },
        locked: block.locked,
    }
}

pub(crate) fn format_timestamp(value: OffsetDateTime) -> String {
    value
        .format(&Rfc3339)
        .unwrap_or_else(|_| value.unix_timestamp().to_string())
}
