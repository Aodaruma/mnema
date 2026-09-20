use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use mnema_app::{
    AddHabitRequest, AppError, AutoPlanChange, AutoScheduleApplyResult, AutoSchedulePreview,
    AutoScheduleRequest, AutoScheduleService, HabitService, ItemScheduleIssue, ScheduleItemRef,
    default_scheduling_preferences,
};
use mnema_core::prelude::*;
use mnema_infra::db::Vault;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    api::{ApiError, ApiResult, AppState, ScheduleBlockDto, format_timestamp, schedule_block_dto},
    config::{ServerConfig, format_date, parse_clock, parse_date, validate_iana_timezone},
};

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/scheduling/preferences",
            get(get_preferences).put(update_preferences),
        )
        .route("/habits", get(list_habits).post(add_habit))
        .route("/habits/{id}", axum::routing::put(update_habit))
        .route("/habits/{id}/disable", post(disable_habit))
        .route("/habits/occurrences/expand", post(expand_occurrences))
        .route("/habits/occurrences/{id}/skip", post(skip_occurrence))
        .route("/habits/occurrences/{id}/snooze", post(snooze_occurrence))
        .route("/auto-schedule/preview", post(preview_seven_days))
        .route("/auto-schedule/apply", post(apply_seven_days))
}

pub(crate) async fn initialize_default_preferences(
    vault: &Vault,
    config: &ServerConfig,
) -> anyhow::Result<()> {
    let repo = vault.scheduling_preferences_repo();
    let user_id = local_user_id();
    if repo.get_for_user(user_id.clone()).await?.is_none() {
        repo.upsert(default_scheduling_preferences(
            user_id,
            config.timezone.clone(),
            OffsetDateTime::now_utc(),
        ))
        .await?;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DailyRangeDto {
    start: String,
    end: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WeekdayRangesDto {
    weekday: String,
    ranges: Vec<DailyRangeDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WeeklyPolicyDto {
    name: String,
    hard: bool,
    days: Vec<WeekdayRangesDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SchedulingPreferencesDto {
    timezone: String,
    named_hours: Vec<WeeklyPolicyDto>,
    sleep: Option<WeeklyPolicyDto>,
    default_travel_buffer_minutes: u32,
    updated_at: Option<String>,
}

async fn get_preferences(
    State(state): State<AppState>,
) -> ApiResult<Json<SchedulingPreferencesDto>> {
    let repo = state.vault.scheduling_preferences_repo();
    let preferences = repo
        .get_for_user(local_user_id())
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::conflict("Scheduling preferences are not configured."))?;
    Ok(Json(preferences_dto(&preferences)))
}

async fn update_preferences(
    State(state): State<AppState>,
    Json(input): Json<SchedulingPreferencesDto>,
) -> ApiResult<Json<SchedulingPreferencesDto>> {
    validate_iana_timezone(&input.timezone).map_err(ApiError::bad_request)?;
    if input.default_travel_buffer_minutes > 360 {
        return Err(ApiError::bad_request(
            "Travel buffer must be between 0 and 360 minutes.",
        ));
    }
    let repo = state.vault.scheduling_preferences_repo();
    let user_id = local_user_id();
    let existing = repo
        .get_for_user(user_id.clone())
        .await
        .map_err(ApiError::internal)?;
    let now = OffsetDateTime::now_utc();
    let preferences = SchedulingPreferences {
        id: existing
            .as_ref()
            .map_or_else(SchedulingPolicyId::new, |value| value.id.clone()),
        user_id,
        timezone: input.timezone.trim().to_string(),
        named_hours: input
            .named_hours
            .into_iter()
            .map(parse_weekly_policy)
            .collect::<ApiResult<_>>()?,
        sleep: input.sleep.map(parse_weekly_policy).transpose()?,
        default_travel_buffer_minutes: input.default_travel_buffer_minutes,
        created_at: existing.as_ref().map_or(now, |value| value.created_at),
        updated_at: now,
    };
    preferences.validate().map_err(ApiError::bad_request)?;
    repo.upsert(preferences.clone())
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(preferences_dto(&preferences)))
}

fn parse_weekly_policy(input: WeeklyPolicyDto) -> ApiResult<WeeklyTimePolicy> {
    Ok(WeeklyTimePolicy {
        name: input.name.trim().to_string(),
        hard: input.hard,
        days: input
            .days
            .into_iter()
            .map(|day| {
                Ok(WeekdayTimeRanges {
                    weekday: parse_weekday(&day.weekday)?,
                    ranges: day
                        .ranges
                        .into_iter()
                        .map(|range| {
                            DailyTimeRange::new(
                                parse_clock(&range.start).map_err(ApiError::bad_request)?,
                                parse_clock(&range.end).map_err(ApiError::bad_request)?,
                            )
                            .map_err(ApiError::bad_request)
                        })
                        .collect::<ApiResult<_>>()?,
                })
            })
            .collect::<ApiResult<_>>()?,
    })
}

fn preferences_dto(preferences: &SchedulingPreferences) -> SchedulingPreferencesDto {
    SchedulingPreferencesDto {
        timezone: preferences.timezone.clone(),
        named_hours: preferences
            .named_hours
            .iter()
            .map(weekly_policy_dto)
            .collect(),
        sleep: preferences.sleep.as_ref().map(weekly_policy_dto),
        default_travel_buffer_minutes: preferences.default_travel_buffer_minutes,
        updated_at: Some(format_timestamp(preferences.updated_at)),
    }
}

fn weekly_policy_dto(policy: &WeeklyTimePolicy) -> WeeklyPolicyDto {
    WeeklyPolicyDto {
        name: policy.name.clone(),
        hard: policy.hard,
        days: policy
            .days
            .iter()
            .map(|day| WeekdayRangesDto {
                weekday: weekday_name(day.weekday).to_string(),
                ranges: day
                    .ranges
                    .iter()
                    .map(|range| DailyRangeDto {
                        start: format!("{:02}:{:02}", range.start.hour(), range.start.minute()),
                        end: format!("{:02}:{:02}", range.end.hour(), range.end.minute()),
                    })
                    .collect(),
            })
            .collect(),
    }
}

fn parse_weekday(value: &str) -> ApiResult<DayOfWeek> {
    match value.trim().to_ascii_uppercase().as_str() {
        "MONDAY" | "MON" => Ok(DayOfWeek::Monday),
        "TUESDAY" | "TUE" => Ok(DayOfWeek::Tuesday),
        "WEDNESDAY" | "WED" => Ok(DayOfWeek::Wednesday),
        "THURSDAY" | "THU" => Ok(DayOfWeek::Thursday),
        "FRIDAY" | "FRI" => Ok(DayOfWeek::Friday),
        "SATURDAY" | "SAT" => Ok(DayOfWeek::Saturday),
        "SUNDAY" | "SUN" => Ok(DayOfWeek::Sunday),
        _ => Err(ApiError::bad_request(format!("Unknown weekday: {value}."))),
    }
}

fn weekday_name(value: DayOfWeek) -> &'static str {
    match value {
        DayOfWeek::Monday => "MONDAY",
        DayOfWeek::Tuesday => "TUESDAY",
        DayOfWeek::Wednesday => "WEDNESDAY",
        DayOfWeek::Thursday => "THURSDAY",
        DayOfWeek::Friday => "FRIDAY",
        DayOfWeek::Saturday => "SATURDAY",
        DayOfWeek::Sunday => "SUNDAY",
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HabitScheduleDto {
    kind: String,
    #[serde(default)]
    weekdays: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct HabitInput {
    title: String,
    schedule: HabitScheduleDto,
    duration_minutes: u32,
    #[serde(default)]
    preferred_window: Option<DailyRangeDto>,
    flexibility: String,
}

#[derive(Debug, Serialize)]
struct HabitDto {
    id: String,
    title: String,
    schedule: HabitScheduleDto,
    duration_minutes: u32,
    preferred_window: Option<DailyRangeDto>,
    flexibility: &'static str,
    enabled: bool,
    disabled_at: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize)]
struct HabitListResponse {
    habits: Vec<HabitDto>,
}

async fn list_habits(State(state): State<AppState>) -> ApiResult<Json<HabitListResponse>> {
    let habits = HabitService::new(
        state.vault.habit_repo().as_ref(),
        state.vault.habit_occurrence_repo().as_ref(),
    )
    .list(local_user_id())
    .await
    .map_err(map_app_error)?;
    Ok(Json(HabitListResponse {
        habits: habits.into_iter().map(habit_dto).collect(),
    }))
}

async fn add_habit(
    State(state): State<AppState>,
    Json(input): Json<HabitInput>,
) -> ApiResult<(axum::http::StatusCode, Json<HabitDto>)> {
    let habit_repo = state.vault.habit_repo();
    let occurrence_repo = state.vault.habit_occurrence_repo();
    let habit = HabitService::new(habit_repo.as_ref(), occurrence_repo.as_ref())
        .add(add_habit_request(input)?)
        .await
        .map_err(map_app_error)?;
    Ok((axum::http::StatusCode::CREATED, Json(habit_dto(habit))))
}

async fn update_habit(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<HabitInput>,
) -> ApiResult<Json<HabitDto>> {
    let habit_id = parse_habit_id(&id)?;
    let repo = state.vault.habit_repo();
    let mut habit = repo
        .find(habit_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::not_found("Habit not found."))?;
    let request = add_habit_request(input)?;
    habit.title = request.title.trim().to_string();
    habit.schedule = request.schedule;
    habit.duration_minutes = request.duration_minutes;
    habit.preferred_window = request.preferred_window;
    habit.flexibility = request.flexibility;
    habit.updated_at = OffsetDateTime::now_utc();
    habit.validate().map_err(ApiError::bad_request)?;
    repo.upsert(habit.clone())
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(habit_dto(habit)))
}

async fn disable_habit(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<HabitDto>> {
    let habit_repo = state.vault.habit_repo();
    let occurrence_repo = state.vault.habit_occurrence_repo();
    let habit = HabitService::new(habit_repo.as_ref(), occurrence_repo.as_ref())
        .disable(parse_habit_id(&id)?)
        .await
        .map_err(map_app_error)?;
    Ok(Json(habit_dto(habit)))
}

fn add_habit_request(input: HabitInput) -> ApiResult<AddHabitRequest> {
    let schedule = match input.schedule.kind.trim().to_ascii_uppercase().as_str() {
        "DAILY" => HabitSchedule::Daily,
        "WEEKDAYS" => HabitSchedule::Weekdays {
            weekdays: input
                .schedule
                .weekdays
                .iter()
                .map(|day| parse_weekday(day))
                .collect::<ApiResult<_>>()?,
        },
        _ => {
            return Err(ApiError::bad_request(
                "Habit schedule kind must be DAILY or WEEKDAYS.",
            ));
        }
    };
    let preferred_window = input
        .preferred_window
        .map(|range| {
            DailyTimeRange::new(
                parse_clock(&range.start).map_err(ApiError::bad_request)?,
                parse_clock(&range.end).map_err(ApiError::bad_request)?,
            )
            .map_err(ApiError::bad_request)
        })
        .transpose()?;
    let flexibility = match input.flexibility.trim().to_ascii_uppercase().as_str() {
        "REQUIRED" => HabitFlexibility::Required,
        "FLEXIBLE" => HabitFlexibility::Flexible,
        _ => {
            return Err(ApiError::bad_request(
                "Habit flexibility must be REQUIRED or FLEXIBLE.",
            ));
        }
    };
    Ok(AddHabitRequest {
        user_id: local_user_id(),
        title: input.title,
        schedule,
        duration_minutes: input.duration_minutes,
        preferred_window,
        flexibility,
    })
}

fn habit_dto(habit: Habit) -> HabitDto {
    let schedule = match habit.schedule {
        HabitSchedule::Daily => HabitScheduleDto {
            kind: "DAILY".to_string(),
            weekdays: Vec::new(),
        },
        HabitSchedule::Weekdays { weekdays } => HabitScheduleDto {
            kind: "WEEKDAYS".to_string(),
            weekdays: weekdays
                .into_iter()
                .map(|day| weekday_name(day).to_string())
                .collect(),
        },
    };
    HabitDto {
        id: habit.id.0.to_string(),
        title: habit.title,
        schedule,
        duration_minutes: habit.duration_minutes,
        preferred_window: habit.preferred_window.map(|range| DailyRangeDto {
            start: format!("{:02}:{:02}", range.start.hour(), range.start.minute()),
            end: format!("{:02}:{:02}", range.end.hour(), range.end.minute()),
        }),
        flexibility: match habit.flexibility {
            HabitFlexibility::Required => "REQUIRED",
            HabitFlexibility::Flexible => "FLEXIBLE",
        },
        enabled: habit.enabled,
        disabled_at: habit.disabled_at.map(format_timestamp),
        created_at: format_timestamp(habit.created_at),
        updated_at: format_timestamp(habit.updated_at),
    }
}

#[derive(Debug, Deserialize)]
struct ExpandOccurrencesInput {
    start_date: String,
    end_date_exclusive: String,
}

#[derive(Debug, Serialize)]
struct OccurrenceDto {
    id: String,
    habit_id: String,
    occurrence_date: String,
    state: &'static str,
    scheduled_start_at: Option<String>,
    scheduled_end_at: Option<String>,
    snoozed_until: Option<String>,
    skip_reason: Option<String>,
}

#[derive(Debug, Serialize)]
struct OccurrenceListResponse {
    occurrences: Vec<OccurrenceDto>,
}

async fn expand_occurrences(
    State(state): State<AppState>,
    Json(input): Json<ExpandOccurrencesInput>,
) -> ApiResult<Json<OccurrenceListResponse>> {
    let habit_repo = state.vault.habit_repo();
    let occurrence_repo = state.vault.habit_occurrence_repo();
    let occurrences = HabitService::new(habit_repo.as_ref(), occurrence_repo.as_ref())
        .expand_occurrences(
            local_user_id(),
            parse_date(&input.start_date).map_err(ApiError::bad_request)?,
            parse_date(&input.end_date_exclusive).map_err(ApiError::bad_request)?,
        )
        .await
        .map_err(map_app_error)?;
    Ok(Json(OccurrenceListResponse {
        occurrences: occurrences.into_iter().map(occurrence_dto).collect(),
    }))
}

#[derive(Debug, Default, Deserialize)]
struct SkipOccurrenceInput {
    #[serde(default)]
    reason: Option<String>,
}

async fn skip_occurrence(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<SkipOccurrenceInput>,
) -> ApiResult<Json<OccurrenceDto>> {
    let habit_repo = state.vault.habit_repo();
    let occurrence_repo = state.vault.habit_occurrence_repo();
    let occurrence = HabitService::new(habit_repo.as_ref(), occurrence_repo.as_ref())
        .skip(parse_occurrence_id(&id)?, input.reason)
        .await
        .map_err(map_app_error)?;
    Ok(Json(occurrence_dto(occurrence)))
}

#[derive(Debug, Deserialize)]
struct SnoozeOccurrenceInput {
    until: String,
}

async fn snooze_occurrence(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<SnoozeOccurrenceInput>,
) -> ApiResult<Json<OccurrenceDto>> {
    let until = OffsetDateTime::parse(
        input.until.trim(),
        &time::format_description::well_known::Rfc3339,
    )
    .map_err(|_| ApiError::bad_request("Snooze time must be an RFC 3339 timestamp."))?;
    let habit_repo = state.vault.habit_repo();
    let occurrence_repo = state.vault.habit_occurrence_repo();
    let occurrence = HabitService::new(habit_repo.as_ref(), occurrence_repo.as_ref())
        .snooze(parse_occurrence_id(&id)?, until)
        .await
        .map_err(map_app_error)?;
    Ok(Json(occurrence_dto(occurrence)))
}

fn occurrence_dto(occurrence: HabitOccurrence) -> OccurrenceDto {
    OccurrenceDto {
        id: occurrence.id.0.to_string(),
        habit_id: occurrence.habit_id.0.to_string(),
        occurrence_date: format_date(occurrence.occurrence_date),
        state: match occurrence.state {
            HabitOccurrenceState::Pending => "PENDING",
            HabitOccurrenceState::Scheduled => "SCHEDULED",
            HabitOccurrenceState::Done => "DONE",
            HabitOccurrenceState::Skipped => "SKIPPED",
            HabitOccurrenceState::Snoozed => "SNOOZED",
        },
        scheduled_start_at: occurrence.scheduled_start_at.map(format_timestamp),
        scheduled_end_at: occurrence.scheduled_end_at.map(format_timestamp),
        snoozed_until: occurrence.snoozed_until.map(format_timestamp),
        skip_reason: occurrence.skip_reason,
    }
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct AutoScheduleInput {
    pub start_date: String,
    #[serde(default = "default_seven_days")]
    pub days: u8,
    #[serde(default)]
    pub timezone: Option<String>,
    #[serde(default)]
    pub named_hours: Vec<String>,
}

const fn default_seven_days() -> u8 {
    7
}

#[derive(Debug, Deserialize)]
struct AutoScheduleApplyInput {
    #[serde(flatten)]
    schedule: AutoScheduleInput,
    fingerprint: String,
}

#[derive(Debug, Serialize)]
struct AutoSchedulePreviewResponse {
    start_date: String,
    end_date_exclusive: String,
    timezone: String,
    fingerprint: String,
    changed_count: usize,
    changes: Vec<AutoPlanChange>,
    blocks: Vec<ScheduleBlockDto>,
    unscheduled: Vec<ScheduleItemRef>,
    issues: Vec<ItemScheduleIssue>,
}

#[derive(Debug, Serialize)]
struct AutoScheduleApplyResponse {
    fingerprint: String,
    blocks: Vec<ScheduleBlockDto>,
}

async fn preview_seven_days(
    State(state): State<AppState>,
    Json(input): Json<AutoScheduleInput>,
) -> ApiResult<Json<AutoSchedulePreviewResponse>> {
    let preview = preview_auto_schedule(&state, resolve_auto_request(&state, input).await?)
        .await
        .map_err(map_app_error)?;
    Ok(Json(auto_preview_response(preview)))
}

async fn apply_seven_days(
    State(state): State<AppState>,
    Json(input): Json<AutoScheduleApplyInput>,
) -> ApiResult<Json<AutoScheduleApplyResponse>> {
    if input.fingerprint.trim().is_empty() {
        return Err(ApiError::bad_request(
            "A preview fingerprint is required before apply.",
        ));
    }
    let request = resolve_auto_request(&state, input.schedule).await?;
    let preview = preview_auto_schedule(&state, request)
        .await
        .map_err(map_app_error)?;
    let result = apply_auto_schedule(&state, &preview, input.fingerprint.trim())
        .await
        .map_err(map_app_error)?;
    Ok(Json(auto_apply_response(result)))
}

async fn resolve_auto_request(
    state: &AppState,
    input: AutoScheduleInput,
) -> ApiResult<AutoScheduleRequest> {
    if !(1..=31).contains(&input.days) {
        return Err(ApiError::bad_request(
            "Auto-schedule days must be between 1 and 31.",
        ));
    }
    let start_date = parse_date(&input.start_date).map_err(ApiError::bad_request)?;
    let end_date_exclusive = start_date
        .checked_add(time::Duration::days(i64::from(input.days)))
        .ok_or_else(|| ApiError::bad_request("Auto-schedule date range overflowed."))?;
    let timezone = if let Some(timezone) = input.timezone {
        timezone
    } else {
        state
            .vault
            .scheduling_preferences_repo()
            .get_for_user(local_user_id())
            .await
            .map_err(ApiError::internal)?
            .map_or_else(|| state.config.timezone.clone(), |value| value.timezone)
    };
    validate_iana_timezone(&timezone).map_err(ApiError::bad_request)?;
    Ok(AutoScheduleRequest {
        user_id: local_user_id(),
        start_date,
        end_date_exclusive,
        timezone,
        named_hours: input.named_hours,
    })
}

pub(crate) async fn preview_auto_schedule(
    state: &AppState,
    request: AutoScheduleRequest,
) -> Result<AutoSchedulePreview, AppError> {
    let task_repo = state.vault.task_repo();
    let status_repo = state.vault.status_repo();
    let schedule_repo = state.vault.schedule_block_repo();
    let event_repo = state.vault.external_event_repo();
    let habit_repo = state.vault.habit_repo();
    let occurrence_repo = state.vault.habit_occurrence_repo();
    let preferences_repo = state.vault.scheduling_preferences_repo();
    AutoScheduleService::new(
        task_repo.as_ref(),
        status_repo.as_ref(),
        schedule_repo.as_ref(),
        event_repo.as_ref(),
        habit_repo.as_ref(),
        occurrence_repo.as_ref(),
        preferences_repo.as_ref(),
    )
    .preview(request)
    .await
}

pub(crate) async fn apply_auto_schedule(
    state: &AppState,
    preview: &AutoSchedulePreview,
    fingerprint: &str,
) -> Result<AutoScheduleApplyResult, AppError> {
    let task_repo = state.vault.task_repo();
    let status_repo = state.vault.status_repo();
    let schedule_repo = state.vault.schedule_block_repo();
    let event_repo = state.vault.external_event_repo();
    let habit_repo = state.vault.habit_repo();
    let occurrence_repo = state.vault.habit_occurrence_repo();
    let preferences_repo = state.vault.scheduling_preferences_repo();
    AutoScheduleService::new(
        task_repo.as_ref(),
        status_repo.as_ref(),
        schedule_repo.as_ref(),
        event_repo.as_ref(),
        habit_repo.as_ref(),
        occurrence_repo.as_ref(),
        preferences_repo.as_ref(),
    )
    .apply(preview, fingerprint)
    .await
}

fn auto_preview_response(preview: AutoSchedulePreview) -> AutoSchedulePreviewResponse {
    let changed_count = preview.diff.changed_count();
    AutoSchedulePreviewResponse {
        start_date: format_date(preview.request.start_date),
        end_date_exclusive: format_date(preview.request.end_date_exclusive),
        timezone: preview.request.timezone,
        fingerprint: preview.diff.fingerprint,
        changed_count,
        changes: preview.diff.changes,
        blocks: preview
            .proposed_blocks
            .into_iter()
            .map(schedule_block_dto)
            .collect(),
        unscheduled: preview.output.unscheduled,
        issues: preview.output.issues,
    }
}

fn auto_apply_response(result: AutoScheduleApplyResult) -> AutoScheduleApplyResponse {
    AutoScheduleApplyResponse {
        fingerprint: result.fingerprint,
        blocks: result.blocks.into_iter().map(schedule_block_dto).collect(),
    }
}

fn parse_habit_id(value: &str) -> ApiResult<HabitId> {
    Uuid::parse_str(value)
        .map(HabitId::from)
        .map_err(|_| ApiError::bad_request("Habit id must be a UUID."))
}

fn parse_occurrence_id(value: &str) -> ApiResult<HabitOccurrenceId> {
    Uuid::parse_str(value)
        .map(HabitOccurrenceId::from)
        .map_err(|_| ApiError::bad_request("Occurrence id must be a UUID."))
}

fn map_app_error(error: AppError) -> ApiError {
    match error {
        AppError::StaleSchedulePreview => ApiError::conflict(error),
        AppError::HabitNotFound | AppError::HabitOccurrenceNotFound => ApiError::not_found(error),
        AppError::MissingSchedulingPreferences => ApiError::conflict(error),
        AppError::Repository(_) => ApiError::internal(error),
        _ => ApiError::bad_request(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::time;

    #[test]
    fn parses_cross_midnight_preferences() {
        let policy = parse_weekly_policy(WeeklyPolicyDto {
            name: "sleep".into(),
            hard: true,
            days: vec![WeekdayRangesDto {
                weekday: "MONDAY".into(),
                ranges: vec![DailyRangeDto {
                    start: "23:00".into(),
                    end: "07:00".into(),
                }],
            }],
        })
        .unwrap();
        assert_eq!(policy.days[0].ranges[0].start, time!(23:00));
        assert!(policy.days[0].ranges[0].crosses_midnight());
    }
}
