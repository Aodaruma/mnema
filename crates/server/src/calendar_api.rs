pub(crate) use mnema_infra::calendar::service::{
    CalendarSyncResponse, CalendarWritebackResponse, is_writeback_account,
    selected_sync_calendar_ids,
};
#[cfg(test)]
use mnema_infra::calendar::service::{managed_provider_event_id, should_full_sync};
use std::{collections::HashMap, sync::Arc};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use mnema_app::iana_date_range;
use mnema_core::prelude::*;
use mnema_infra::calendar::{
    CalendarError, CalendarReadApi, CalendarWriteApi, CredentialStore, GoogleCalendarAdapter,
    GoogleCalendarConfig, OAuthPkce, RemoteCalendar,
};
use serde::{Deserialize, Serialize};
use time::{Date, Duration, OffsetDateTime};
use time_tz::{OffsetDateTimeExt, timezones};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{
    api::{ApiError, ApiResult, AppState},
    config::parse_date,
};

#[derive(Clone)]
pub struct CalendarRuntime {
    google: Result<Arc<GoogleCalendarAdapter>, Arc<str>>,
    credentials: Arc<dyn CredentialStore>,
    pending_oauth: Arc<Mutex<HashMap<String, PendingOAuth>>>,
}

#[derive(Clone)]
struct PendingOAuth {
    pkce: OAuthPkce,
    account_id: CalendarAccountId,
    access_mode: CalendarAccessMode,
    created_at: OffsetDateTime,
}

impl CalendarRuntime {
    pub fn from_env(credentials: Arc<dyn CredentialStore>) -> Self {
        let google = GoogleCalendarConfig::from_env()
            .map(|config| Arc::new(GoogleCalendarAdapter::new(config, credentials.clone())))
            .map_err(|error| Arc::<str>::from(error.to_string()));
        Self {
            google,
            credentials,
            pending_oauth: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn is_configured(&self) -> bool {
        self.google.is_ok()
    }

    fn google(&self) -> ApiResult<Arc<GoogleCalendarAdapter>> {
        self.google.clone().map_err(|message| {
            ApiError::service_unavailable(format!(
                "Google Calendar is not configured: {message}. Set MNEMA_GOOGLE_CLIENT_ID and MNEMA_GOOGLE_REDIRECT_URI."
            ))
        })
    }
}

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/calendar/oauth/start", post(start_oauth))
        .route("/calendar/oauth/callback", get(oauth_callback))
        .route("/calendar/accounts", get(list_accounts))
        .route(
            "/calendar/accounts/{id}/calendars",
            get(list_remote_calendars),
        )
        .route(
            "/calendar/accounts/{id}/selection",
            axum::routing::put(update_calendar_selection),
        )
        .route("/calendar/accounts/{id}/sync", post(sync_account))
        .route(
            "/calendar/accounts/{id}/managed-calendar",
            post(ensure_managed_calendar),
        )
        .route("/calendar/accounts/{id}/writeback", post(writeback_account))
}

#[derive(Debug, Default, Deserialize)]
struct OAuthStartInput {
    #[serde(default)]
    access_mode: Option<String>,
}

#[derive(Debug, Serialize)]
struct OAuthStartResponse {
    authorization_url: String,
    state: String,
    access_mode: &'static str,
}

async fn start_oauth(
    State(state): State<AppState>,
    Json(input): Json<OAuthStartInput>,
) -> ApiResult<Json<OAuthStartResponse>> {
    let adapter = state.calendar.google()?;
    let access_mode = parse_access_mode(input.access_mode.as_deref())?;
    let pkce = OAuthPkce::generate();
    let authorization_url = adapter
        .authorization_url_for_pkce(&pkce, access_mode)
        .map_err(map_calendar_error)?;
    let state_token = pkce.state().to_string();
    let pending = PendingOAuth {
        pkce,
        account_id: CalendarAccountId::new(),
        access_mode,
        created_at: OffsetDateTime::now_utc(),
    };
    let mut pending_oauth = state.calendar.pending_oauth.lock().await;
    pending_oauth
        .retain(|_, value| OffsetDateTime::now_utc() - value.created_at < Duration::minutes(15));
    pending_oauth.insert(state_token.clone(), pending);
    Ok(Json(OAuthStartResponse {
        authorization_url: authorization_url.to_string(),
        state: state_token,
        access_mode: access_mode_name(access_mode),
    }))
}

#[derive(Debug, Default, Deserialize)]
struct OAuthCallbackQuery {
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

async fn oauth_callback(
    State(state): State<AppState>,
    Query(query): Query<OAuthCallbackQuery>,
) -> Response {
    let result = complete_oauth(&state, query).await;
    match result {
        Ok(account) => oauth_html(
            StatusCode::OK,
            "Google Calendar connected",
            &format!("{} を接続しました。", account.display_name),
            true,
        ),
        Err(error) => oauth_html(
            error.status,
            "Google Calendar connection failed",
            &error.message,
            false,
        ),
    }
}

async fn complete_oauth(state: &AppState, query: OAuthCallbackQuery) -> ApiResult<CalendarAccount> {
    let state_token = query
        .state
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ApiError::bad_request("OAuth callback state is missing."))?;
    let pending = state
        .calendar
        .pending_oauth
        .lock()
        .await
        .remove(&state_token)
        .ok_or_else(|| ApiError::bad_request("OAuth state is unknown or expired."))?;
    if !pending.pkce.matches_state(&state_token)
        || OffsetDateTime::now_utc() - pending.created_at >= Duration::minutes(15)
    {
        return Err(ApiError::bad_request("OAuth state is invalid or expired."));
    }
    if let Some(provider_error) = query.error {
        let description = query.error_description.unwrap_or_default();
        return Err(ApiError::bad_request(format!(
            "Google authorization failed: {provider_error} {description}"
        )));
    }
    let code = query
        .code
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ApiError::bad_request("OAuth authorization code is missing."))?;
    let adapter = state.calendar.google()?;
    adapter
        .exchange_code(
            pending.account_id.clone(),
            &code,
            pending.pkce.code_verifier(),
        )
        .await
        .map_err(map_calendar_error)?;

    let now = OffsetDateTime::now_utc();
    let provisional = CalendarAccount {
        id: pending.account_id.clone(),
        user_id: local_user_id(),
        provider: CalendarProvider::Google,
        provider_account_id: pending.account_id.0.to_string(),
        display_name: "Google Calendar".to_string(),
        email: None,
        access_mode: pending.access_mode,
        enabled: true,
        selected_calendar_ids: Vec::new(),
        managed_calendar_id: None,
        timezone: None,
        created_at: now,
        updated_at: now,
    };
    let calendars = match adapter.list_calendars(&provisional).await {
        Ok(calendars) => calendars,
        Err(error) => {
            let _ = state
                .calendar
                .credentials
                .delete(pending.account_id.clone())
                .await;
            return Err(map_calendar_error(error));
        }
    };
    let primary = calendars
        .iter()
        .find(|calendar| calendar.primary)
        .or_else(|| calendars.first());
    let provider_identity = primary
        .map(|calendar| calendar.id.clone())
        .unwrap_or_else(|| pending.account_id.0.to_string());
    let repo = state.vault.calendar_account_repo();
    let existing = repo
        .find_by_provider_identity(CalendarProvider::Google, provider_identity.clone())
        .await
        .map_err(ApiError::internal)?;
    let mut account = provisional;
    account.provider_account_id = provider_identity.clone();
    account.display_name = primary
        .map(|calendar| calendar.summary.clone())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "Google Calendar".to_string());
    account.email = provider_identity.contains('@').then_some(provider_identity);
    account.timezone = primary.and_then(|calendar| calendar.timezone.clone());
    account.selected_calendar_ids = calendars
        .iter()
        .filter(|calendar| calendar.primary || calendar.selected)
        .map(|calendar| calendar.id.clone())
        .collect();

    if let Some(existing) = existing {
        if existing.id != pending.account_id {
            if let Some(credential) = state
                .calendar
                .credentials
                .load(pending.account_id.clone())
                .await
                .map_err(ApiError::internal)?
            {
                state
                    .calendar
                    .credentials
                    .save(existing.id.clone(), credential)
                    .await
                    .map_err(ApiError::internal)?;
            }
            state
                .calendar
                .credentials
                .delete(pending.account_id)
                .await
                .map_err(ApiError::internal)?;
        }
        account.id = existing.id;
        account.created_at = existing.created_at;
        account.managed_calendar_id = existing.managed_calendar_id;
        if !existing.selected_calendar_ids.is_empty() {
            account.selected_calendar_ids = existing.selected_calendar_ids;
        }
    }
    repo.upsert(account.clone())
        .await
        .map_err(ApiError::internal)?;
    Ok(account)
}

fn oauth_html(status: StatusCode, title: &str, message: &str, redirect: bool) -> Response {
    let redirect_script = if redirect {
        "<script>setTimeout(() => location.replace('/#calendar'), 900)</script>"
    } else {
        ""
    };
    (
        status,
        Html(format!(
            "<!doctype html><html lang=\"ja\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\"><title>{}</title><body style=\"font:16px system-ui;max-width:620px;margin:12vh auto;padding:24px\"><h1>{}</h1><p>{}</p><p><a href=\"/#calendar\">Mnema に戻る</a></p>{}</body></html>",
            escape_html(title),
            escape_html(title),
            escape_html(message),
            redirect_script
        )),
    )
        .into_response()
}

#[derive(Debug, Serialize)]
struct AccountListResponse {
    accounts: Vec<CalendarAccount>,
    google_configured: bool,
}

async fn list_accounts(State(state): State<AppState>) -> ApiResult<Json<AccountListResponse>> {
    let accounts = state
        .vault
        .calendar_account_repo()
        .list_enabled(local_user_id())
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(AccountListResponse {
        accounts,
        google_configured: state.calendar.is_configured(),
    }))
}

#[derive(Debug, Serialize)]
struct RemoteCalendarListResponse {
    calendars: Vec<RemoteCalendar>,
}

async fn list_remote_calendars(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<RemoteCalendarListResponse>> {
    let account = find_account(&state, &id).await?;
    let calendars = state
        .calendar
        .google()?
        .list_calendars(&account)
        .await
        .map_err(map_calendar_error)?;
    Ok(Json(RemoteCalendarListResponse { calendars }))
}

#[derive(Debug, Deserialize)]
struct CalendarSelectionInput {
    calendar_ids: Vec<String>,
}

async fn update_calendar_selection(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<CalendarSelectionInput>,
) -> ApiResult<Json<CalendarAccount>> {
    if input.calendar_ids.len() > 100 {
        return Err(ApiError::bad_request(
            "At most 100 calendars can be selected.",
        ));
    }
    let mut selected = input
        .calendar_ids
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    selected.sort();
    selected.dedup();
    let mut account = find_account(&state, &id).await?;
    if let Some(managed_calendar_id) = account.managed_calendar_id.as_deref() {
        selected.retain(|calendar_id| calendar_id != managed_calendar_id);
    }
    account.selected_calendar_ids = selected;
    account.updated_at = OffsetDateTime::now_utc();
    state
        .vault
        .calendar_account_repo()
        .upsert(account.clone())
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(account))
}

#[derive(Debug, Clone, Default, Deserialize)]
struct CalendarRangeInput {
    #[serde(default)]
    start_date: Option<String>,
    #[serde(default)]
    end_date_exclusive: Option<String>,
    #[serde(default)]
    force_full: bool,
}

async fn sync_account(
    State(state): State<AppState>,
    Path(id): Path<String>,
    input: Option<Json<CalendarRangeInput>>,
) -> ApiResult<Json<CalendarSyncResponse>> {
    let account = find_account(&state, &id).await?;
    let input = input.map(|value| value.0);
    let force_full = input.as_ref().is_some_and(|value| value.force_full);
    let (start, end, _) = resolve_calendar_range(&state, input).await?;
    Ok(Json(
        sync_selected_calendars(&state, &account, start, end, force_full).await?,
    ))
}

pub(crate) async fn enabled_calendar_accounts(state: &AppState) -> ApiResult<Vec<CalendarAccount>> {
    state
        .vault
        .calendar_account_repo()
        .list_enabled(local_user_id())
        .await
        .map_err(ApiError::internal)
}

pub(crate) async fn sync_selected_calendars(
    state: &AppState,
    account: &CalendarAccount,
    start: Date,
    end: Date,
    force_full: bool,
) -> ApiResult<CalendarSyncResponse> {
    if selected_sync_calendar_ids(account).is_empty() {
        return Err(ApiError::conflict(
            "Select at least one provider calendar before sync.",
        ));
    }
    let window = iana_date_range(start, end, &account_timezone(state, account).await?)
        .map_err(ApiError::bad_request)?;
    mnema_infra::calendar::service::sync_selected_calendars(
        &state.vault,
        state.calendar.google()?.as_ref(),
        account,
        window.start,
        window.end,
        force_full,
    )
    .await
    .map_err(map_calendar_service_error)
}

#[derive(Debug, Default, Deserialize)]
struct ManagedCalendarInput {
    #[serde(default)]
    summary: Option<String>,
}

async fn ensure_managed_calendar(
    State(state): State<AppState>,
    Path(id): Path<String>,
    input: Option<Json<ManagedCalendarInput>>,
) -> ApiResult<Json<CalendarAccount>> {
    let mut account = find_account(&state, &id).await?;
    let summary = input
        .and_then(|value| value.0.summary)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "Mnema Schedule".to_string());
    let managed = state
        .calendar
        .google()?
        .ensure_managed_calendar(&account, &summary)
        .await
        .map_err(map_calendar_error)?;
    account
        .selected_calendar_ids
        .retain(|calendar_id| calendar_id != &managed.id);
    account.managed_calendar_id = Some(managed.id);
    account.updated_at = OffsetDateTime::now_utc();
    state
        .vault
        .calendar_account_repo()
        .upsert(account.clone())
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(account))
}

async fn writeback_account(
    State(state): State<AppState>,
    Path(id): Path<String>,
    input: Option<Json<CalendarRangeInput>>,
) -> ApiResult<Json<CalendarWritebackResponse>> {
    let account = find_account(&state, &id).await?;
    let (start, end, timezone) = resolve_calendar_range(&state, input.map(|value| value.0)).await?;
    Ok(Json(
        writeback_schedule(&state, &account, start, end, &timezone).await?,
    ))
}

pub(crate) async fn writeback_schedule(
    state: &AppState,
    account: &CalendarAccount,
    start: Date,
    end: Date,
    timezone: &str,
) -> ApiResult<CalendarWritebackResponse> {
    if account.access_mode != CalendarAccessMode::ReadWrite {
        return Err(ApiError::forbidden(
            "Calendar account does not allow write-back.",
        ));
    }
    if account
        .managed_calendar_id
        .as_deref()
        .is_none_or(|id| id.trim().is_empty())
    {
        return Err(ApiError::conflict(
            "Create a managed calendar before write-back.",
        ));
    }
    let window = iana_date_range(start, end, timezone).map_err(ApiError::bad_request)?;
    mnema_infra::calendar::service::writeback_schedule(
        &state.vault,
        state.calendar.google()?.as_ref(),
        account,
        window.start,
        window.end,
        timezone,
    )
    .await
    .map_err(map_calendar_service_error)
}

async fn resolve_calendar_range(
    state: &AppState,
    input: Option<CalendarRangeInput>,
) -> ApiResult<(Date, Date, String)> {
    let preferences = state
        .vault
        .scheduling_preferences_repo()
        .get_for_user(local_user_id())
        .await
        .map_err(ApiError::internal)?;
    let timezone = preferences
        .map(|value| value.timezone)
        .unwrap_or_else(|| state.config.timezone.clone());
    let timezone_ref = timezones::get_by_name(&timezone)
        .ok_or_else(|| ApiError::bad_request("Configured IANA timezone is invalid."))?;
    let input = input.unwrap_or_default();
    let start = input
        .start_date
        .filter(|value| !value.trim().is_empty())
        .map(|value| parse_date(&value).map_err(ApiError::bad_request))
        .transpose()?
        .unwrap_or_else(|| OffsetDateTime::now_utc().to_timezone(timezone_ref).date());
    let end = input
        .end_date_exclusive
        .filter(|value| !value.trim().is_empty())
        .map(|value| parse_date(&value).map_err(ApiError::bad_request))
        .transpose()?
        .unwrap_or_else(|| start.checked_add(Duration::days(8)).unwrap_or(start));
    if start >= end {
        return Err(ApiError::bad_request(
            "Calendar range start must be before end.",
        ));
    }
    Ok((start, end, timezone))
}

async fn account_timezone(state: &AppState, account: &CalendarAccount) -> ApiResult<String> {
    if let Some(timezone) = &account.timezone
        && timezones::get_by_name(timezone).is_some()
    {
        return Ok(timezone.clone());
    }
    Ok(state
        .vault
        .scheduling_preferences_repo()
        .get_for_user(local_user_id())
        .await
        .map_err(ApiError::internal)?
        .map_or_else(|| state.config.timezone.clone(), |value| value.timezone))
}

async fn find_account(state: &AppState, value: &str) -> ApiResult<CalendarAccount> {
    let account_id = Uuid::parse_str(value)
        .map(CalendarAccountId::from)
        .map_err(|_| ApiError::bad_request("Calendar account id must be a UUID."))?;
    state
        .vault
        .calendar_account_repo()
        .find(account_id)
        .await
        .map_err(ApiError::internal)?
        .filter(|account| account.user_id == local_user_id() && account.enabled)
        .ok_or_else(|| ApiError::not_found("Calendar account not found."))
}

fn parse_access_mode(value: Option<&str>) -> ApiResult<CalendarAccessMode> {
    match value
        .unwrap_or("read_only")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "read_only" | "readonly" | "read" => Ok(CalendarAccessMode::ReadOnly),
        "read_write" | "readwrite" | "write" => Ok(CalendarAccessMode::ReadWrite),
        _ => Err(ApiError::bad_request(
            "Calendar access_mode must be read_only or read_write.",
        )),
    }
}

fn access_mode_name(value: CalendarAccessMode) -> &'static str {
    match value {
        CalendarAccessMode::ReadOnly => "read_only",
        CalendarAccessMode::ReadWrite => "read_write",
    }
}

fn map_calendar_service_error(error: anyhow::Error) -> ApiError {
    match error.downcast::<CalendarError>() {
        Ok(error) => map_calendar_error(error),
        Err(error) => ApiError::internal(error),
    }
}

fn map_calendar_error(error: CalendarError) -> ApiError {
    match error {
        CalendarError::MissingConfiguration(_) => ApiError::service_unavailable(error),
        CalendarError::MissingCredential
        | CalendarError::MissingRefreshToken
        | CalendarError::Unauthorized => ApiError::unauthorized(error),
        CalendarError::WriteNotAllowed => ApiError::forbidden(error),
        CalendarError::InvalidRange
        | CalendarError::InvalidResponse(_)
        | CalendarError::Url(_)
        | CalendarError::UnsupportedProvider => ApiError::bad_request(error),
        CalendarError::Repository(_) | CalendarError::Credential(_) => ApiError::internal(error),
        CalendarError::Http(_)
        | CalendarError::Provider { .. }
        | CalendarError::SyncTokenExpired
        | CalendarError::FullSyncRequired => ApiError::bad_gateway(error),
    }
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calendar_account(
        access_mode: CalendarAccessMode,
        enabled: bool,
        managed_calendar_id: Option<&str>,
    ) -> CalendarAccount {
        let now = OffsetDateTime::now_utc();
        CalendarAccount {
            id: CalendarAccountId::new(),
            user_id: local_user_id(),
            provider: CalendarProvider::Google,
            provider_account_id: "test@example.com".into(),
            display_name: "Test account".into(),
            email: Some("test@example.com".into()),
            access_mode,
            enabled,
            selected_calendar_ids: vec!["primary".into(), "managed".into(), " ".into()],
            managed_calendar_id: managed_calendar_id.map(str::to_string),
            timezone: Some("Etc/UTC".into()),
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn access_mode_defaults_to_read_only() {
        assert_eq!(
            parse_access_mode(None).unwrap(),
            CalendarAccessMode::ReadOnly
        );
        assert_eq!(
            parse_access_mode(Some("read_write")).unwrap(),
            CalendarAccessMode::ReadWrite
        );
    }

    #[test]
    fn html_messages_are_escaped() {
        assert_eq!(escape_html("<token>&\""), "&lt;token&gt;&amp;&quot;");
    }

    #[test]
    fn date_format_is_stable_for_sync_responses() {
        assert_eq!(
            crate::config::format_date(
                Date::from_calendar_date(2026, time::Month::August, 15).unwrap()
            ),
            "2026-08-15"
        );
    }

    #[test]
    fn selected_sync_calendars_exclude_managed_and_blank_ids() {
        let account = calendar_account(CalendarAccessMode::ReadWrite, true, Some("managed"));

        assert_eq!(selected_sync_calendar_ids(&account), vec!["primary"]);
    }

    #[test]
    fn writeback_requires_enabled_read_write_account_with_managed_calendar() {
        assert!(is_writeback_account(&calendar_account(
            CalendarAccessMode::ReadWrite,
            true,
            Some("managed")
        )));
        assert!(!is_writeback_account(&calendar_account(
            CalendarAccessMode::ReadOnly,
            true,
            Some("managed")
        )));
        assert!(!is_writeback_account(&calendar_account(
            CalendarAccessMode::ReadWrite,
            false,
            Some("managed")
        )));
        assert!(!is_writeback_account(&calendar_account(
            CalendarAccessMode::ReadWrite,
            true,
            None
        )));
        assert!(!is_writeback_account(&calendar_account(
            CalendarAccessMode::ReadWrite,
            true,
            Some(" ")
        )));
    }

    #[test]
    fn sync_refreshes_the_rolling_horizon_after_twelve_hours_or_when_forced() {
        let now = time::macros::datetime!(2026-08-15 12:00 UTC);
        let stale = CalendarSyncCursor {
            account_id: CalendarAccountId::new(),
            calendar_id: "primary".into(),
            sync_token: Some("token".into()),
            last_full_sync_at: Some(now - Duration::hours(13)),
            last_incremental_sync_at: None,
            updated_at: now,
        };
        assert!(should_full_sync(false, Some(&stale), now));

        let fresh = CalendarSyncCursor {
            last_full_sync_at: Some(now - Duration::hours(1)),
            ..stale
        };
        assert!(!should_full_sync(false, Some(&fresh), now));
        assert!(should_full_sync(true, Some(&fresh), now));
        let missing_token = CalendarSyncCursor {
            sync_token: None,
            ..fresh
        };
        assert!(should_full_sync(false, Some(&missing_token), now));
        assert!(should_full_sync(false, None, now));
    }

    #[test]
    fn managed_event_id_is_stable_lowercase_and_hyphen_free() {
        let block_id =
            ScheduleBlockId::from(Uuid::parse_str("123E4567-E89B-12D3-A456-426614174000").unwrap());

        assert_eq!(
            managed_provider_event_id(&block_id),
            "mnema123e4567e89b12d3a456426614174000"
        );
    }
}
