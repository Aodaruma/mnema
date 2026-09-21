use std::{fmt, sync::Arc};

use mnema_core::prelude::*;
use reqwest::header::{HeaderValue, IF_MATCH};
use reqwest::{Client, Method, StatusCode, Url};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use time::format_description::well_known::Rfc3339;
use time::{Date, Duration, OffsetDateTime, PrimitiveDateTime, UtcOffset};
use time_tz::{OffsetResult, PrimitiveDateTimeExt, timezones};

use super::{
    CalendarError, CalendarEventChange, CalendarEventDraft, CalendarEventListMode,
    CalendarEventListRequest, CalendarEventPage, CalendarEventTombstone, CalendarReadApi,
    CalendarSyncReport, CalendarWriteApi, CredentialStore, OAuthCredential, OAuthPkce,
    RemoteCalendar,
};

const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const GOOGLE_CALENDAR_API_BASE_URL: &str = "https://www.googleapis.com/calendar/v3/";
const GOOGLE_READ_ONLY_SCOPE: &str = "https://www.googleapis.com/auth/calendar.readonly";
const GOOGLE_EVENT_WRITE_SCOPE: &str = "https://www.googleapis.com/auth/calendar.events";
const GOOGLE_CALENDAR_MANAGE_SCOPE: &str = "https://www.googleapis.com/auth/calendar";
const GOOGLE_CALENDAR_LIST_SCOPE: &str =
    "https://www.googleapis.com/auth/calendar.calendarlist.readonly";

#[derive(Clone, PartialEq, Eq)]
pub struct GoogleCalendarConfig {
    pub client_id: String,
    pub client_secret: Option<String>,
    pub redirect_uri: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub api_base_url: String,
}

impl fmt::Debug for GoogleCalendarConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GoogleCalendarConfig")
            .field("client_id", &self.client_id)
            .field(
                "client_secret",
                &self.client_secret.as_ref().map(|_| "[REDACTED]"),
            )
            .field("redirect_uri", &self.redirect_uri)
            .field("authorization_endpoint", &self.authorization_endpoint)
            .field("token_endpoint", &self.token_endpoint)
            .field("api_base_url", &self.api_base_url)
            .finish()
    }
}

impl GoogleCalendarConfig {
    pub fn from_env() -> Result<Self, CalendarError> {
        let client_id = std::env::var("MNEMA_GOOGLE_CLIENT_ID")
            .map_err(|_| CalendarError::MissingConfiguration("MNEMA_GOOGLE_CLIENT_ID".into()))?;
        let redirect_uri = std::env::var("MNEMA_GOOGLE_REDIRECT_URI")
            .map_err(|_| CalendarError::MissingConfiguration("MNEMA_GOOGLE_REDIRECT_URI".into()))?;
        Ok(Self {
            client_id,
            client_secret: std::env::var("MNEMA_GOOGLE_CLIENT_SECRET").ok(),
            redirect_uri,
            authorization_endpoint: std::env::var("MNEMA_GOOGLE_AUTH_URL")
                .unwrap_or_else(|_| GOOGLE_AUTH_URL.into()),
            token_endpoint: std::env::var("MNEMA_GOOGLE_TOKEN_URL")
                .unwrap_or_else(|_| GOOGLE_TOKEN_URL.into()),
            api_base_url: std::env::var("MNEMA_GOOGLE_CALENDAR_API_BASE_URL")
                .unwrap_or_else(|_| GOOGLE_CALENDAR_API_BASE_URL.into()),
        })
    }
}

#[derive(Clone)]
pub struct GoogleCalendarAdapter {
    http: Client,
    config: GoogleCalendarConfig,
    credentials: Arc<dyn CredentialStore>,
}

impl GoogleCalendarAdapter {
    #[must_use]
    pub fn new(config: GoogleCalendarConfig, credentials: Arc<dyn CredentialStore>) -> Self {
        Self {
            http: Client::builder()
                .connect_timeout(std::time::Duration::from_secs(10))
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("HTTP client configuration is valid"),
            config,
            credentials,
        }
    }

    #[must_use]
    pub fn with_client(
        config: GoogleCalendarConfig,
        credentials: Arc<dyn CredentialStore>,
        http: Client,
    ) -> Self {
        Self {
            http,
            config,
            credentials,
        }
    }

    pub fn authorization_url(
        &self,
        state: &str,
        access_mode: CalendarAccessMode,
        code_challenge: &str,
    ) -> Result<Url, CalendarError> {
        if state.trim().is_empty() || code_challenge.trim().is_empty() {
            return Err(CalendarError::InvalidResponse(
                "OAuth state and PKCE challenge are required".into(),
            ));
        }
        let mut url = Url::parse(&self.config.authorization_endpoint)
            .map_err(|error| CalendarError::Url(error.to_string()))?;
        let scope = google_scope(access_mode);
        url.query_pairs_mut()
            .append_pair("client_id", &self.config.client_id)
            .append_pair("redirect_uri", &self.config.redirect_uri)
            .append_pair("response_type", "code")
            .append_pair("scope", &scope)
            .append_pair("access_type", "offline")
            .append_pair("include_granted_scopes", "true")
            .append_pair("prompt", "consent")
            .append_pair("state", state)
            .append_pair("code_challenge", code_challenge)
            .append_pair("code_challenge_method", "S256");
        Ok(url)
    }

    pub fn authorization_url_for_pkce(
        &self,
        pkce: &OAuthPkce,
        access_mode: CalendarAccessMode,
    ) -> Result<Url, CalendarError> {
        self.authorization_url(pkce.state(), access_mode, pkce.code_challenge())
    }

    pub async fn exchange_code(
        &self,
        account_id: CalendarAccountId,
        code: &str,
        code_verifier: &str,
    ) -> Result<OAuthCredential, CalendarError> {
        let mut form = vec![
            ("client_id", self.config.client_id.clone()),
            ("code", code.to_string()),
            ("code_verifier", code_verifier.to_string()),
            ("grant_type", "authorization_code".into()),
            ("redirect_uri", self.config.redirect_uri.clone()),
        ];
        if let Some(secret) = &self.config.client_secret {
            form.push(("client_secret", secret.clone()));
        }

        let response = self
            .http
            .post(&self.config.token_endpoint)
            .form(&form)
            .send()
            .await?;
        let token = parse_token_response(response, None).await?;
        self.credentials.save(account_id, token.clone()).await?;
        Ok(token)
    }

    pub async fn refresh_access_token(
        &self,
        account_id: CalendarAccountId,
    ) -> Result<OAuthCredential, CalendarError> {
        let previous = self
            .credentials
            .load(account_id.clone())
            .await?
            .ok_or(CalendarError::MissingCredential)?;
        let refresh_token = previous
            .refresh_token
            .clone()
            .ok_or(CalendarError::MissingRefreshToken)?;
        let mut form = vec![
            ("client_id", self.config.client_id.clone()),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token".into()),
        ];
        if let Some(secret) = &self.config.client_secret {
            form.push(("client_secret", secret.clone()));
        }

        let response = self
            .send_with_retry(
                self.http.post(&self.config.token_endpoint).form(&form),
                true,
            )
            .await?;
        let token = parse_token_response(response, Some(previous)).await?;
        self.credentials.save(account_id, token.clone()).await?;
        Ok(token)
    }

    async fn access_token(&self, account_id: CalendarAccountId) -> Result<String, CalendarError> {
        let credential = self
            .credentials
            .load(account_id.clone())
            .await?
            .ok_or(CalendarError::MissingCredential)?;
        if credential.expires_soon(OffsetDateTime::now_utc()) {
            return Ok(self.refresh_access_token(account_id).await?.access_token);
        }
        Ok(credential.access_token)
    }

    async fn get(
        &self,
        account_id: CalendarAccountId,
        url: Url,
    ) -> Result<reqwest::Response, CalendarError> {
        let access_token = self.access_token(account_id.clone()).await?;
        let response = self
            .send_with_retry(self.http.get(url.clone()).bearer_auth(access_token), true)
            .await?;
        if response.status() != StatusCode::UNAUTHORIZED {
            return Ok(response);
        }
        let refreshed = self.refresh_access_token(account_id).await?;
        self.send_with_retry(self.http.get(url).bearer_auth(refreshed.access_token), true)
            .await
    }

    async fn send_json(
        &self,
        account_id: CalendarAccountId,
        method: Method,
        url: Url,
        body: Option<Value>,
        expected_etag: Option<&str>,
    ) -> Result<reqwest::Response, CalendarError> {
        let access_token = self.access_token(account_id.clone()).await?;
        let retry_safe =
            method != Method::POST || body.as_ref().and_then(|body| body.get("id")).is_some();
        let response = self
            .send_with_retry(
                self.json_request(
                    method.clone(),
                    url.clone(),
                    &access_token,
                    body.as_ref(),
                    expected_etag,
                )?,
                retry_safe,
            )
            .await?;
        if response.status() != StatusCode::UNAUTHORIZED {
            return Ok(response);
        }
        let refreshed = self.refresh_access_token(account_id).await?;
        self.send_with_retry(
            self.json_request(
                method,
                url,
                &refreshed.access_token,
                body.as_ref(),
                expected_etag,
            )?,
            retry_safe,
        )
        .await
    }

    /// Bounded exponential backoff. Never replay a non-idempotent calendar
    /// creation or OAuth code exchange after an ambiguous transport failure.
    async fn send_with_retry(
        &self,
        request: reqwest::RequestBuilder,
        retry_safe: bool,
    ) -> Result<reqwest::Response, CalendarError> {
        for attempt in 0..4_u32 {
            let result = request
                .try_clone()
                .ok_or_else(|| {
                    CalendarError::InvalidResponse("request body is not replayable".into())
                })?
                .timeout(std::time::Duration::from_secs(30))
                .send()
                .await;
            let mut retry_after = None;
            match result {
                Ok(response) => {
                    let status = response.status();
                    let transient =
                        status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error();
                    if !retry_safe || (!transient && status != StatusCode::FORBIDDEN) {
                        return Ok(response);
                    }
                    retry_after = response
                        .headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|value| value.to_str().ok())
                        .and_then(|value| value.parse::<u64>().ok());
                    let body = response.text().await?;
                    let rate_limited =
                        status == StatusCode::FORBIDDEN && google_rate_limited(&body);
                    if attempt == 3 || (!transient && !rate_limited) {
                        return Err(CalendarError::Provider {
                            status: status.as_u16(),
                            body,
                        });
                    }
                }
                Err(error) => {
                    if !retry_safe
                        || attempt == 3
                        || !(error.is_timeout() || error.is_connect() || error.is_request())
                    {
                        return Err(CalendarError::Http(error.without_url()));
                    }
                }
            }
            let jitter = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_millis() as u64;
            let millis = retry_after
                .map(|seconds| seconds.saturating_mul(1000))
                .unwrap_or((500_u64 << attempt) + jitter)
                .min(30_000);
            tokio::time::sleep(std::time::Duration::from_millis(millis)).await;
        }
        unreachable!("retry loop returns after its final attempt")
    }

    fn json_request(
        &self,
        method: Method,
        url: Url,
        access_token: &str,
        body: Option<&Value>,
        expected_etag: Option<&str>,
    ) -> Result<reqwest::RequestBuilder, CalendarError> {
        let mut request = self.http.request(method, url).bearer_auth(access_token);
        if let Some(body) = body {
            request = request.json(body);
        }
        if let Some(etag) = expected_etag {
            request = request.header(
                IF_MATCH,
                HeaderValue::from_str(etag)
                    .map_err(|error| CalendarError::InvalidResponse(error.to_string()))?,
            );
        }
        Ok(request)
    }

    fn api_url(&self, segments: &[&str]) -> Result<Url, CalendarError> {
        let mut url = Url::parse(&self.config.api_base_url)
            .map_err(|error| CalendarError::Url(error.to_string()))?;
        {
            let mut path = url
                .path_segments_mut()
                .map_err(|_| CalendarError::Url("Google API base URL cannot be a base".into()))?;
            path.pop_if_empty();
            for segment in segments {
                path.push(segment);
            }
        }
        Ok(url)
    }

    pub async fn full_sync(
        &self,
        account: &CalendarAccount,
        calendar_id: &str,
        time_min: Option<OffsetDateTime>,
        time_max: Option<OffsetDateTime>,
        events: &dyn ExternalEventRepository,
        cursors: &dyn CalendarSyncCursorRepository,
    ) -> Result<CalendarSyncReport, CalendarError> {
        if time_min
            .zip(time_max)
            .is_some_and(|(start, end)| start >= end)
        {
            return Err(CalendarError::InvalidRange);
        }
        self.sync(
            account,
            calendar_id,
            CalendarEventListMode::Full { time_min, time_max },
            None,
            events,
            cursors,
        )
        .await
    }

    pub async fn incremental_sync(
        &self,
        account: &CalendarAccount,
        calendar_id: &str,
        events: &dyn ExternalEventRepository,
        cursors: &dyn CalendarSyncCursorRepository,
    ) -> Result<CalendarSyncReport, CalendarError> {
        let cursor = cursors
            .get(account.id.clone(), calendar_id.to_string())
            .await?
            .ok_or(CalendarError::FullSyncRequired)?;
        let sync_token = cursor
            .sync_token
            .clone()
            .ok_or(CalendarError::FullSyncRequired)?;
        let result = self
            .sync(
                account,
                calendar_id,
                CalendarEventListMode::Incremental { sync_token },
                Some(cursor),
                events,
                cursors,
            )
            .await;
        if matches!(result, Err(CalendarError::SyncTokenExpired)) {
            cursors
                .clear(account.id.clone(), calendar_id.to_string())
                .await?;
        }
        result
    }

    async fn sync(
        &self,
        account: &CalendarAccount,
        calendar_id: &str,
        mode: CalendarEventListMode,
        previous_cursor: Option<CalendarSyncCursor>,
        events: &dyn ExternalEventRepository,
        cursors: &dyn CalendarSyncCursorRepository,
    ) -> Result<CalendarSyncReport, CalendarError> {
        let is_full = matches!(mode, CalendarEventListMode::Full { .. });
        let mut page_token: Option<String> = None;
        let mut report = CalendarSyncReport::default();
        let mut full_snapshot_event_ids = Vec::new();

        loop {
            let page = self
                .list_event_page(
                    account,
                    CalendarEventListRequest {
                        calendar_id: calendar_id.to_string(),
                        mode: mode.clone(),
                        page_token: page_token.clone(),
                    },
                )
                .await?;
            report.pages += 1;

            for change in page.changes {
                match change {
                    CalendarEventChange::Upsert(mut event) => {
                        if is_full {
                            full_snapshot_event_ids.push(event.provider_event_id.clone());
                        }
                        if let Some(existing) = events
                            .find_by_provider_event(
                                event.account_id.clone(),
                                event.calendar_id.clone(),
                                event.provider_event_id.clone(),
                            )
                            .await?
                        {
                            event.id = existing.id;
                            event.created_at = existing.created_at;
                        }
                        events.upsert(*event).await?;
                        report.upserted += 1;
                    }
                    CalendarEventChange::Cancelled(tombstone) => {
                        if is_full {
                            full_snapshot_event_ids.push(tombstone.provider_event_id.clone());
                        }
                        if let Some(mut existing) = events
                            .find_by_provider_event(
                                account.id.clone(),
                                calendar_id.to_string(),
                                tombstone.provider_event_id,
                            )
                            .await?
                        {
                            existing.status = ExternalEventStatus::Cancelled;
                            existing.etag = tombstone.etag;
                            existing.provider_updated_at = tombstone.provider_updated_at;
                            existing.updated_at = OffsetDateTime::now_utc();
                            events.upsert(existing).await?;
                            report.cancelled += 1;
                        }
                    }
                }
            }

            if let Some(next_page_token) = page.next_page_token {
                page_token = Some(next_page_token);
                continue;
            }
            report.next_sync_token = page.next_sync_token;
            break;
        }

        let sync_token = report.next_sync_token.clone().ok_or_else(|| {
            CalendarError::InvalidResponse("final events page did not contain nextSyncToken".into())
        })?;
        if is_full {
            events
                .delete_unseen_for_calendar(
                    account.id.clone(),
                    calendar_id.to_string(),
                    full_snapshot_event_ids,
                )
                .await?;
        }
        let now = OffsetDateTime::now_utc();
        cursors
            .upsert(CalendarSyncCursor {
                account_id: account.id.clone(),
                calendar_id: calendar_id.to_string(),
                sync_token: Some(sync_token),
                last_full_sync_at: if is_full {
                    Some(now)
                } else {
                    previous_cursor
                        .as_ref()
                        .and_then(|cursor| cursor.last_full_sync_at)
                },
                last_incremental_sync_at: if is_full { None } else { Some(now) },
                updated_at: now,
            })
            .await?;
        Ok(report)
    }

    fn ensure_google_account(account: &CalendarAccount) -> Result<(), CalendarError> {
        if account.provider != CalendarProvider::Google {
            return Err(CalendarError::UnsupportedProvider);
        }
        Ok(())
    }

    fn ensure_write_access(account: &CalendarAccount) -> Result<(), CalendarError> {
        Self::ensure_google_account(account)?;
        if account.access_mode != CalendarAccessMode::ReadWrite {
            return Err(CalendarError::WriteNotAllowed);
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl CalendarReadApi for GoogleCalendarAdapter {
    async fn list_calendars(
        &self,
        account: &CalendarAccount,
    ) -> Result<Vec<RemoteCalendar>, CalendarError> {
        Self::ensure_google_account(account)?;
        let mut page_token: Option<String> = None;
        let mut calendars = Vec::new();
        loop {
            let mut url = self.api_url(&["users", "me", "calendarList"])?;
            url.query_pairs_mut().append_pair("maxResults", "250");
            if let Some(token) = &page_token {
                url.query_pairs_mut().append_pair("pageToken", token);
            }
            let response = self.get(account.id.clone(), url).await?;
            let response = checked_response(response).await?;
            let page: GoogleCalendarList = response.json().await?;
            calendars.extend(page.items.into_iter().map(|calendar| RemoteCalendar {
                id: calendar.id,
                summary: calendar.summary,
                primary: calendar.primary,
                selected: calendar.selected,
                access_role: calendar.access_role,
                timezone: calendar.time_zone,
            }));
            if let Some(next) = page.next_page_token {
                page_token = Some(next);
            } else {
                break;
            }
        }
        Ok(calendars)
    }

    async fn list_event_page(
        &self,
        account: &CalendarAccount,
        request: CalendarEventListRequest,
    ) -> Result<CalendarEventPage, CalendarError> {
        Self::ensure_google_account(account)?;
        let mut url = self.api_url(&["calendars", &request.calendar_id, "events"])?;
        {
            let mut query = url.query_pairs_mut();
            query
                .append_pair("maxResults", "2500")
                .append_pair("showDeleted", "true")
                .append_pair("singleEvents", "true");
            match &request.mode {
                CalendarEventListMode::Full { time_min, time_max } => {
                    if let Some(time_min) = time_min {
                        query.append_pair("timeMin", &format_rfc3339(*time_min)?);
                    }
                    if let Some(time_max) = time_max {
                        query.append_pair("timeMax", &format_rfc3339(*time_max)?);
                    }
                }
                CalendarEventListMode::Incremental { sync_token } => {
                    query.append_pair("syncToken", sync_token);
                }
            }
            if let Some(page_token) = &request.page_token {
                query.append_pair("pageToken", page_token);
            }
        }

        let response = self.get(account.id.clone(), url).await?;
        if response.status() == StatusCode::GONE {
            return Err(CalendarError::SyncTokenExpired);
        }
        let response = checked_response(response).await?;
        let page: GoogleEventList = response.json().await?;
        let fallback_timezone = page.time_zone.as_deref().or(account.timezone.as_deref());
        let now = OffsetDateTime::now_utc();
        let changes = page
            .items
            .into_iter()
            .map(|event| {
                google_event_change_with_timezone(
                    account,
                    &request.calendar_id,
                    event,
                    now,
                    fallback_timezone,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(CalendarEventPage {
            changes,
            next_page_token: page.next_page_token,
            next_sync_token: page.next_sync_token,
        })
    }
}

#[async_trait::async_trait]
impl CalendarWriteApi for GoogleCalendarAdapter {
    async fn ensure_managed_calendar(
        &self,
        account: &CalendarAccount,
        summary: &str,
    ) -> Result<RemoteCalendar, CalendarError> {
        Self::ensure_write_access(account)?;
        if let Some(calendar_id) = &account.managed_calendar_id {
            let url = self.api_url(&["calendars", calendar_id])?;
            let response = self.get(account.id.clone(), url).await?;
            if !matches!(response.status(), StatusCode::NOT_FOUND | StatusCode::GONE) {
                let response = checked_response(response).await?;
                let calendar: GoogleCalendarListEntry = response.json().await?;
                return Ok(RemoteCalendar {
                    id: calendar.id,
                    summary: if calendar.summary.is_empty() {
                        summary.to_owned()
                    } else {
                        calendar.summary
                    },
                    primary: calendar.primary,
                    selected: true,
                    access_role: calendar.access_role.or_else(|| Some("owner".into())),
                    timezone: calendar.time_zone.or_else(|| account.timezone.clone()),
                });
            }
        }
        if summary.trim().is_empty() {
            return Err(CalendarError::InvalidResponse(
                "managed calendar summary must not be empty".into(),
            ));
        }
        let url = self.api_url(&["calendars"])?;
        let response = self
            .send_json(
                account.id.clone(),
                Method::POST,
                url,
                Some(json!({ "summary": summary })),
                None,
            )
            .await?;
        let response = checked_response(response).await?;
        let calendar: GoogleCalendarListEntry = response.json().await?;
        Ok(RemoteCalendar {
            id: calendar.id,
            summary: calendar.summary,
            primary: calendar.primary,
            selected: calendar.selected,
            access_role: calendar.access_role.or_else(|| Some("owner".into())),
            timezone: calendar.time_zone,
        })
    }

    async fn create_event(
        &self,
        account: &CalendarAccount,
        calendar_id: &str,
        draft: CalendarEventDraft,
    ) -> Result<ExternalEvent, CalendarError> {
        Self::ensure_write_access(account)?;
        let requested_event_id = draft.provider_event_id.clone();
        let body = google_event_create_body(&draft)?;
        let url = self.api_url(&["calendars", calendar_id, "events"])?;
        let response = self
            .send_json(account.id.clone(), Method::POST, url, Some(body), None)
            .await?;
        if response.status() == StatusCode::CONFLICT
            && let Some(provider_event_id) = requested_event_id
        {
            let url = self.api_url(&["calendars", calendar_id, "events", &provider_event_id])?;
            let response = self.get(account.id.clone(), url).await?;
            let response = checked_response(response).await?;
            let event: GoogleEvent = response.json().await?;
            return match google_event_change(
                account,
                calendar_id,
                event,
                OffsetDateTime::now_utc(),
            )? {
                CalendarEventChange::Upsert(event) => Ok(*event),
                CalendarEventChange::Cancelled(_) => Err(CalendarError::InvalidResponse(
                    "idempotently recovered event was returned as cancelled".into(),
                )),
            };
        }
        let response = checked_response(response).await?;
        let event: GoogleEvent = response.json().await?;
        match google_event_change(account, calendar_id, event, OffsetDateTime::now_utc())? {
            CalendarEventChange::Upsert(event) => Ok(*event),
            CalendarEventChange::Cancelled(_) => Err(CalendarError::InvalidResponse(
                "created event was returned as cancelled".into(),
            )),
        }
    }

    async fn update_event(
        &self,
        account: &CalendarAccount,
        calendar_id: &str,
        provider_event_id: &str,
        expected_etag: Option<&str>,
        draft: CalendarEventDraft,
    ) -> Result<ExternalEvent, CalendarError> {
        Self::ensure_write_access(account)?;
        let body = google_event_body(&draft)?;
        let url = self.api_url(&["calendars", calendar_id, "events", provider_event_id])?;
        let response = self
            .send_json(
                account.id.clone(),
                Method::PATCH,
                url,
                Some(body),
                expected_etag,
            )
            .await?;
        let response = checked_response(response).await?;
        let event: GoogleEvent = response.json().await?;
        match google_event_change(account, calendar_id, event, OffsetDateTime::now_utc())? {
            CalendarEventChange::Upsert(event) => Ok(*event),
            CalendarEventChange::Cancelled(_) => Err(CalendarError::InvalidResponse(
                "updated event was returned as cancelled".into(),
            )),
        }
    }

    async fn delete_event(
        &self,
        account: &CalendarAccount,
        calendar_id: &str,
        provider_event_id: &str,
        expected_etag: Option<&str>,
    ) -> Result<(), CalendarError> {
        Self::ensure_write_access(account)?;
        let url = self.api_url(&["calendars", calendar_id, "events", provider_event_id])?;
        let response = self
            .send_json(account.id.clone(), Method::DELETE, url, None, expected_etag)
            .await?;
        checked_response(response).await?;
        Ok(())
    }
}

fn google_rate_limited(body: &str) -> bool {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .pointer("/error/errors")
                .and_then(Value::as_array)
                .map(|errors| {
                    errors.iter().any(|error| {
                        matches!(
                            error.get("reason").and_then(Value::as_str),
                            Some("rateLimitExceeded" | "userRateLimitExceeded")
                        )
                    })
                })
        })
        .unwrap_or(false)
}

fn google_scope(access_mode: CalendarAccessMode) -> String {
    match access_mode {
        CalendarAccessMode::ReadOnly => GOOGLE_READ_ONLY_SCOPE.into(),
        CalendarAccessMode::ReadWrite => {
            format!(
                "{GOOGLE_EVENT_WRITE_SCOPE} {GOOGLE_CALENDAR_LIST_SCOPE} {GOOGLE_CALENDAR_MANAGE_SCOPE}"
            )
        }
    }
}

async fn checked_response(response: reqwest::Response) -> Result<reqwest::Response, CalendarError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    if status == StatusCode::UNAUTHORIZED {
        return Err(CalendarError::Unauthorized);
    }
    if status == StatusCode::GONE {
        return Err(CalendarError::SyncTokenExpired);
    }
    let body = response.text().await.unwrap_or_default();
    Err(CalendarError::Provider {
        status: status.as_u16(),
        body,
    })
}

#[derive(Debug, Deserialize)]
struct GoogleTokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default = "default_token_type")]
    token_type: String,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
}

fn default_token_type() -> String {
    "Bearer".into()
}

async fn parse_token_response(
    response: reqwest::Response,
    previous: Option<OAuthCredential>,
) -> Result<OAuthCredential, CalendarError> {
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        if status == StatusCode::BAD_REQUEST && body.contains("invalid_grant") {
            return Err(CalendarError::Unauthorized);
        }
        return Err(CalendarError::Provider {
            status: status.as_u16(),
            body,
        });
    }
    let response: GoogleTokenResponse = response.json().await?;
    let expires_at = response
        .expires_in
        .map(|seconds| OffsetDateTime::now_utc() + Duration::seconds(seconds));
    Ok(OAuthCredential {
        access_token: response.access_token,
        refresh_token: response.refresh_token.or_else(|| {
            previous
                .as_ref()
                .and_then(|value| value.refresh_token.clone())
        }),
        token_type: response.token_type,
        scope: response
            .scope
            .or_else(|| previous.as_ref().and_then(|value| value.scope.clone())),
        expires_at,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleCalendarList {
    #[serde(default)]
    items: Vec<GoogleCalendarListEntry>,
    next_page_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleCalendarListEntry {
    id: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    primary: bool,
    #[serde(default)]
    selected: bool,
    access_role: Option<String>,
    time_zone: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleEventList {
    #[serde(default)]
    items: Vec<GoogleEvent>,
    next_page_token: Option<String>,
    next_sync_token: Option<String>,
    time_zone: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct GoogleEvent {
    id: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    location: Option<String>,
    #[serde(default)]
    start: Option<GoogleEventDateTime>,
    #[serde(default)]
    end: Option<GoogleEventDateTime>,
    #[serde(default)]
    recurring_event_id: Option<String>,
    #[serde(default)]
    original_start_time: Option<GoogleEventDateTime>,
    #[serde(default)]
    transparency: Option<String>,
    #[serde(default)]
    etag: Option<String>,
    #[serde(default)]
    updated: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct GoogleEventDateTime {
    #[serde(default)]
    date_time: Option<String>,
    #[serde(default)]
    date: Option<String>,
    #[serde(default)]
    time_zone: Option<String>,
}

fn google_event_change(
    account: &CalendarAccount,
    calendar_id: &str,
    event: GoogleEvent,
    now: OffsetDateTime,
) -> Result<CalendarEventChange, CalendarError> {
    google_event_change_with_timezone(
        account,
        calendar_id,
        event,
        now,
        account.timezone.as_deref(),
    )
}

fn google_event_change_with_timezone(
    account: &CalendarAccount,
    calendar_id: &str,
    event: GoogleEvent,
    now: OffsetDateTime,
    fallback_timezone: Option<&str>,
) -> Result<CalendarEventChange, CalendarError> {
    let provider_updated_at = event.updated.as_deref().map(parse_rfc3339).transpose()?;
    let cancelled = event.status.as_deref() == Some("cancelled");
    let Some(start_value) = event.start else {
        if cancelled {
            return Ok(CalendarEventChange::Cancelled(CalendarEventTombstone {
                provider_event_id: event.id,
                etag: event.etag,
                provider_updated_at,
            }));
        }
        return Err(CalendarError::InvalidResponse(format!(
            "event {} has no start",
            event.id
        )));
    };
    let end_value = event
        .end
        .ok_or_else(|| CalendarError::InvalidResponse(format!("event {} has no end", event.id)))?;
    let (start_at, start_all_day, start_timezone) =
        parse_google_event_time(&start_value, fallback_timezone)?;
    let (end_at, end_all_day, end_timezone) =
        parse_google_event_time(&end_value, fallback_timezone)?;
    if end_at <= start_at {
        return Err(CalendarError::InvalidResponse(format!(
            "event {} has an invalid time range",
            event.id
        )));
    }
    let original_start_at = event
        .original_start_time
        .as_ref()
        .map(|value| parse_google_event_time(value, fallback_timezone))
        .transpose()?
        .map(|(value, _, _)| value);

    Ok(CalendarEventChange::Upsert(Box::new(ExternalEvent {
        id: ExternalEventId::new(),
        account_id: account.id.clone(),
        calendar_id: calendar_id.to_string(),
        provider_event_id: event.id,
        recurring_event_id: event.recurring_event_id,
        original_start_at,
        title: event.summary.unwrap_or_else(|| "(untitled event)".into()),
        description: event.description,
        location: event.location,
        start_at,
        end_at,
        all_day: start_all_day && end_all_day,
        timezone: start_timezone.or(end_timezone),
        status: match event.status.as_deref() {
            Some("tentative") => ExternalEventStatus::Tentative,
            Some("cancelled") => ExternalEventStatus::Cancelled,
            _ => ExternalEventStatus::Confirmed,
        },
        transparency: if event.transparency.as_deref() == Some("transparent") {
            ExternalEventTransparency::Transparent
        } else {
            ExternalEventTransparency::Opaque
        },
        needs_travel: None,
        travel_before_minutes: None,
        travel_after_minutes: None,
        etag: event.etag,
        provider_updated_at,
        created_at: now,
        updated_at: now,
    })))
}

fn parse_google_event_time(
    value: &GoogleEventDateTime,
    fallback_timezone: Option<&str>,
) -> Result<(OffsetDateTime, bool, Option<String>), CalendarError> {
    if let Some(date_time) = &value.date_time {
        if let Ok(parsed) = OffsetDateTime::parse(date_time, &Rfc3339) {
            return Ok((parsed, false, value.time_zone.clone()));
        }
        let timezone_name = value
            .time_zone
            .as_deref()
            .or(fallback_timezone)
            .ok_or_else(|| {
                CalendarError::InvalidResponse(
                    "offset-less event dateTime requires an IANA timeZone".into(),
                )
            })?;
        let timezone = timezones::get_by_name(timezone_name).ok_or_else(|| {
            CalendarError::InvalidResponse(format!(
                "unknown IANA timezone for event dateTime: {timezone_name}"
            ))
        })?;
        let local = parse_google_local_datetime(date_time)?;
        let parsed = match local.assume_timezone(timezone) {
            OffsetResult::Some(value) => value,
            OffsetResult::Ambiguous(first, second) => first.min(second),
            OffsetResult::None => {
                return Err(CalendarError::InvalidResponse(format!(
                    "event dateTime is invalid in {timezone_name}"
                )));
            }
        };
        return Ok((parsed, false, Some(timezone_name.to_owned())));
    }
    if let Some(date) = &value.date {
        let date = Date::parse(
            date,
            time::macros::format_description!("[year]-[month]-[day]"),
        )
        .map_err(|error| CalendarError::InvalidResponse(error.to_string()))?;
        let local_midnight = date
            .with_hms(0, 0, 0)
            .map_err(|error| CalendarError::InvalidResponse(error.to_string()))?;
        let timezone_name = value.time_zone.as_deref().or(fallback_timezone);
        let start = if let Some(timezone_name) = timezone_name {
            let timezone = timezones::get_by_name(timezone_name).ok_or_else(|| {
                CalendarError::InvalidResponse(format!(
                    "unknown IANA timezone for all-day event: {timezone_name}"
                ))
            })?;
            match local_midnight.assume_timezone(timezone) {
                OffsetResult::Some(value) => value,
                OffsetResult::Ambiguous(first, second) => first.min(second),
                OffsetResult::None => {
                    return Err(CalendarError::InvalidResponse(format!(
                        "all-day event starts at an invalid local time in {timezone_name}"
                    )));
                }
            }
        } else {
            local_midnight.assume_offset(UtcOffset::UTC)
        };
        return Ok((start, true, timezone_name.map(ToOwned::to_owned)));
    }
    Err(CalendarError::InvalidResponse(
        "event time has neither dateTime nor date".into(),
    ))
}

fn parse_google_local_datetime(value: &str) -> Result<PrimitiveDateTime, CalendarError> {
    let with_subseconds = time::macros::format_description!(
        "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond]"
    );
    let without_subseconds =
        time::macros::format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]");
    PrimitiveDateTime::parse(value, with_subseconds)
        .or_else(|_| PrimitiveDateTime::parse(value, without_subseconds))
        .map_err(|error| CalendarError::InvalidResponse(error.to_string()))
}

fn google_event_body(draft: &CalendarEventDraft) -> Result<Value, CalendarError> {
    if draft.end_at <= draft.start_at {
        return Err(CalendarError::InvalidRange);
    }
    let (start, end) = if draft.all_day {
        (
            json!({
                "date": draft.start_at.date().to_string(),
            }),
            json!({
                "date": draft.end_at.date().to_string(),
            }),
        )
    } else {
        (
            json!({
                "dateTime": format_rfc3339(draft.start_at)?,
                "timeZone": draft.timezone,
            }),
            json!({
                "dateTime": format_rfc3339(draft.end_at)?,
                "timeZone": draft.timezone,
            }),
        )
    };
    Ok(json!({
        "summary": draft.title,
        "description": draft.description,
        "location": draft.location,
        "start": start,
        "end": end,
        "extendedProperties": {
            "private": {
                "mnemaManaged": "true"
            }
        }
    }))
}

fn google_event_create_body(draft: &CalendarEventDraft) -> Result<Value, CalendarError> {
    let mut body = google_event_body(draft)?;
    if let Some(provider_event_id) = &draft.provider_event_id {
        validate_google_event_id(provider_event_id)?;
        body.as_object_mut()
            .expect("Google event body is always a JSON object")
            .insert("id".into(), Value::String(provider_event_id.clone()));
    }
    Ok(body)
}

fn validate_google_event_id(provider_event_id: &str) -> Result<(), CalendarError> {
    let valid_length = (5..=1024).contains(&provider_event_id.len());
    let valid_characters = provider_event_id
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'v').contains(&byte));
    if valid_length && valid_characters {
        return Ok(());
    }
    Err(CalendarError::InvalidResponse(
        "Google event id must be 5-1024 lowercase base32hex characters (a-v, 0-9)".into(),
    ))
}

fn parse_rfc3339(value: &str) -> Result<OffsetDateTime, CalendarError> {
    OffsetDateTime::parse(value, &Rfc3339)
        .map_err(|error| CalendarError::InvalidResponse(error.to_string()))
}

fn format_rfc3339(value: OffsetDateTime) -> Result<String, CalendarError> {
    value
        .format(&Rfc3339)
        .map_err(|error| CalendarError::InvalidResponse(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::MemoryCredentialStore;
    use time::macros::datetime;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    async fn mock_google_api(
        responses: Vec<(u16, &'static str)>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = vec![0_u8; 4096];
                let _ = stream.read(&mut request).await.unwrap();
                let reason = match status {
                    200 => "OK",
                    404 => "Not Found",
                    _ => "Error",
                };
                let response = format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });
        (format!("http://{address}/calendar/v3/"), task)
    }

    async fn adapter_for_mock(
        account: &CalendarAccount,
        responses: Vec<(u16, &'static str)>,
    ) -> (GoogleCalendarAdapter, tokio::task::JoinHandle<()>) {
        let (api_base_url, task) = mock_google_api(responses).await;
        let credentials = Arc::new(MemoryCredentialStore::default());
        credentials
            .save(
                account.id.clone(),
                OAuthCredential {
                    access_token: "test-access-token".into(),
                    refresh_token: None,
                    token_type: "Bearer".into(),
                    scope: None,
                    expires_at: None,
                },
            )
            .await
            .unwrap();
        let mut config = config();
        config.api_base_url = api_base_url;
        (GoogleCalendarAdapter::new(config, credentials), task)
    }

    fn config() -> GoogleCalendarConfig {
        GoogleCalendarConfig {
            client_id: "client-id".into(),
            client_secret: None,
            redirect_uri: "http://127.0.0.1:45678/callback".into(),
            authorization_endpoint: GOOGLE_AUTH_URL.into(),
            token_endpoint: GOOGLE_TOKEN_URL.into(),
            api_base_url: GOOGLE_CALENDAR_API_BASE_URL.into(),
        }
    }

    #[tokio::test]
    async fn transient_and_rate_limit_errors_retry_but_permission_errors_do_not() {
        let account = account();
        for status in [429, 503, 403] {
            let (adapter, server) = adapter_for_mock(
                &account,
                vec![
                    (
                        status,
                        r#"{"error":{"errors":[{"reason":"rateLimitExceeded"}]}}"#,
                    ),
                    (200, r#"{"items":[]}"#),
                ],
            )
            .await;
            assert!(adapter.list_calendars(&account).await.unwrap().is_empty());
            server.await.unwrap();
        }
        let (adapter, server) = adapter_for_mock(
            &account,
            vec![(403, r#"{"error":{"errors":[{"reason":"forbidden"}]}}"#)],
        )
        .await;
        assert!(matches!(
            adapter.list_calendars(&account).await,
            Err(CalendarError::Provider { status: 403, .. })
        ));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn calendar_creation_is_not_replayed_on_ambiguous_failure() {
        let mut account = account();
        account.managed_calendar_id = None;
        let (adapter, server) = adapter_for_mock(&account, vec![(503, "unavailable")]).await;
        assert!(matches!(
            adapter.ensure_managed_calendar(&account, "Mnema").await,
            Err(CalendarError::Provider { status: 503, .. })
        ));
        server.await.unwrap();
    }

    fn account() -> CalendarAccount {
        let now = datetime!(2026-08-15 00:00 UTC);
        CalendarAccount {
            id: CalendarAccountId::new(),
            user_id: UserId::new(),
            provider: CalendarProvider::Google,
            provider_account_id: "person@example.com".into(),
            display_name: "Personal".into(),
            email: Some("person@example.com".into()),
            access_mode: CalendarAccessMode::ReadWrite,
            enabled: true,
            selected_calendar_ids: vec!["primary".into()],
            managed_calendar_id: Some("primary".into()),
            timezone: Some("Asia/Tokyo".into()),
            created_at: now,
            updated_at: now,
        }
    }

    fn timed_draft(provider_event_id: Option<&str>) -> CalendarEventDraft {
        CalendarEventDraft {
            provider_event_id: provider_event_id.map(ToOwned::to_owned),
            title: "Focus".into(),
            description: Some("Managed by Mnema".into()),
            location: None,
            start_at: datetime!(2026-08-15 09:00 +09:00),
            end_at: datetime!(2026-08-15 10:00 +09:00),
            all_day: false,
            timezone: Some("Asia/Tokyo".into()),
        }
    }

    #[test]
    fn authorization_url_uses_pkce_and_requested_scope() {
        let adapter =
            GoogleCalendarAdapter::new(config(), Arc::new(MemoryCredentialStore::default()));
        let url = adapter
            .authorization_url("state-value", CalendarAccessMode::ReadOnly, "challenge")
            .unwrap();
        let query = url
            .query_pairs()
            .collect::<std::collections::HashMap<_, _>>();

        assert_eq!(query.get("code_challenge_method").unwrap(), "S256");
        assert_eq!(query.get("access_type").unwrap(), "offline");
        assert_eq!(query.get("scope").unwrap(), GOOGLE_READ_ONLY_SCOPE);
    }

    #[test]
    fn authorization_url_accepts_generated_pkce_material() {
        let adapter =
            GoogleCalendarAdapter::new(config(), Arc::new(MemoryCredentialStore::default()));
        let pkce = OAuthPkce::generate();
        let url = adapter
            .authorization_url_for_pkce(&pkce, CalendarAccessMode::ReadWrite)
            .unwrap();
        let query = url
            .query_pairs()
            .collect::<std::collections::HashMap<_, _>>();
        assert_eq!(query.get("state").unwrap(), pkce.state());
        assert_eq!(query.get("code_challenge").unwrap(), pkce.code_challenge());
        assert!(
            query
                .get("scope")
                .unwrap()
                .contains(GOOGLE_CALENDAR_MANAGE_SCOPE)
        );
    }

    #[test]
    fn config_debug_redacts_client_secret() {
        let mut config = config();
        config.client_secret = Some("never-log-this-secret".into());
        let debug = format!("{config:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("never-log-this-secret"));
    }

    #[tokio::test]
    async fn managed_calendar_id_is_verified_before_reuse() {
        let account = account();
        let (adapter, server) = adapter_for_mock(
            &account,
            vec![(
                200,
                r#"{"id":"primary","summary":"Mnema","timeZone":"Asia/Tokyo"}"#,
            )],
        )
        .await;
        let calendar = adapter
            .ensure_managed_calendar(&account, "Mnema")
            .await
            .unwrap();
        server.await.unwrap();
        assert_eq!(calendar.id, "primary");
        assert_eq!(calendar.summary, "Mnema");
    }

    #[tokio::test]
    async fn deleted_managed_calendar_is_recreated() {
        let account = account();
        let (adapter, server) = adapter_for_mock(
            &account,
            vec![
                (404, r#"{"error":"not found"}"#),
                (
                    200,
                    r#"{"id":"replacement","summary":"Mnema","timeZone":"Asia/Tokyo"}"#,
                ),
            ],
        )
        .await;
        let calendar = adapter
            .ensure_managed_calendar(&account, "Mnema")
            .await
            .unwrap();
        server.await.unwrap();
        assert_eq!(calendar.id, "replacement");
    }

    #[tokio::test]
    async fn deterministic_create_conflict_fetches_the_existing_event() {
        let account = account();
        let provider_event_id = "mnema0123456789abcdef";
        let (adapter, server) = adapter_for_mock(
            &account,
            vec![
                (409, r#"{"error":"duplicate"}"#),
                (
                    200,
                    r#"{
                        "id":"mnema0123456789abcdef",
                        "status":"confirmed",
                        "summary":"Focus",
                        "start":{"dateTime":"2026-08-15T09:00:00+09:00","timeZone":"Asia/Tokyo"},
                        "end":{"dateTime":"2026-08-15T10:00:00+09:00","timeZone":"Asia/Tokyo"}
                    }"#,
                ),
            ],
        )
        .await;

        let event = adapter
            .create_event(
                &account,
                "mnema-managed",
                timed_draft(Some(provider_event_id)),
            )
            .await
            .unwrap();
        server.await.unwrap();
        assert_eq!(event.provider_event_id, provider_event_id);
        assert_eq!(event.title, "Focus");
    }

    #[test]
    fn parses_timed_google_event_without_local_overrides() {
        let event: GoogleEvent = serde_json::from_value(json!({
            "id": "provider-event",
            "status": "confirmed",
            "summary": "Meeting",
            "location": "Office",
            "start": { "dateTime": "2026-08-15T09:00:00+09:00", "timeZone": "Asia/Tokyo" },
            "end": { "dateTime": "2026-08-15T10:00:00+09:00", "timeZone": "Asia/Tokyo" },
            "etag": "etag-1",
            "updated": "2026-08-15T00:00:00Z"
        }))
        .unwrap();

        let change = google_event_change(
            &account(),
            "primary",
            event,
            datetime!(2026-08-15 00:00 UTC),
        )
        .unwrap();
        let CalendarEventChange::Upsert(event) = change else {
            panic!("expected upsert");
        };
        assert_eq!(event.location.as_deref(), Some("Office"));
        assert_eq!(event.needs_travel, None);
        assert_eq!(event.start_at, datetime!(2026-08-15 09:00 +09:00));
    }

    #[test]
    fn parses_offsetless_datetime_with_its_iana_timezone() {
        let event: GoogleEvent = serde_json::from_value(json!({
            "id": "provider-event",
            "status": "confirmed",
            "start": { "dateTime": "2026-08-15T09:00:00", "timeZone": "Asia/Tokyo" },
            "end": { "dateTime": "2026-08-15T10:00:00", "timeZone": "Asia/Tokyo" }
        }))
        .unwrap();

        let change = google_event_change(
            &account(),
            "primary",
            event,
            datetime!(2026-08-15 00:00 UTC),
        )
        .unwrap();
        let CalendarEventChange::Upsert(event) = change else {
            panic!("expected upsert");
        };
        assert_eq!(event.start_at, datetime!(2026-08-15 09:00 +09:00));
        assert_eq!(event.end_at, datetime!(2026-08-15 10:00 +09:00));
        assert_eq!(event.timezone.as_deref(), Some("Asia/Tokyo"));
    }

    #[test]
    fn cancelled_tombstone_does_not_require_event_times() {
        let event: GoogleEvent = serde_json::from_value(json!({
            "id": "deleted-event",
            "status": "cancelled"
        }))
        .unwrap();

        assert!(matches!(
            google_event_change(
                &account(),
                "primary",
                event,
                datetime!(2026-08-15 00:00 UTC)
            )
            .unwrap(),
            CalendarEventChange::Cancelled(_)
        ));
    }

    #[test]
    fn all_day_events_use_the_account_timezone() {
        let event: GoogleEvent = serde_json::from_value(json!({
            "id": "all-day",
            "status": "confirmed",
            "summary": "Holiday",
            "start": { "date": "2026-08-15" },
            "end": { "date": "2026-08-16" }
        }))
        .unwrap();
        let change = google_event_change(
            &account(),
            "primary",
            event,
            datetime!(2026-08-15 00:00 UTC),
        )
        .unwrap();
        let CalendarEventChange::Upsert(event) = change else {
            panic!("expected upsert");
        };
        assert!(event.all_day);
        assert_eq!(event.start_at, datetime!(2026-08-15 00:00 +09:00));
        assert_eq!(event.end_at, datetime!(2026-08-16 00:00 +09:00));
    }

    #[test]
    fn all_day_events_prefer_the_event_page_calendar_timezone() {
        let page: GoogleEventList = serde_json::from_value(json!({
            "timeZone": "America/New_York",
            "items": [{
                "id": "all-day-secondary",
                "status": "confirmed",
                "start": { "date": "2026-08-15" },
                "end": { "date": "2026-08-16" }
            }]
        }))
        .unwrap();
        let account = account();
        let fallback_timezone = page.time_zone.as_deref().or(account.timezone.as_deref());
        let event = page.items.into_iter().next().unwrap();
        let change = google_event_change_with_timezone(
            &account,
            "secondary",
            event,
            datetime!(2026-08-15 00:00 UTC),
            fallback_timezone,
        )
        .unwrap();
        let CalendarEventChange::Upsert(event) = change else {
            panic!("expected upsert");
        };
        assert_eq!(event.start_at, datetime!(2026-08-15 00:00 -04:00));
        assert_eq!(event.end_at, datetime!(2026-08-16 00:00 -04:00));
        assert_eq!(event.timezone.as_deref(), Some("America/New_York"));
    }

    #[test]
    fn all_day_write_preserves_the_drafts_local_date() {
        let body = google_event_body(&CalendarEventDraft {
            provider_event_id: None,
            title: "Holiday".into(),
            description: None,
            location: None,
            start_at: datetime!(2026-08-15 00:00 +09:00),
            end_at: datetime!(2026-08-16 00:00 +09:00),
            all_day: true,
            timezone: Some("Asia/Tokyo".into()),
        })
        .unwrap();
        assert_eq!(body["start"]["date"], "2026-08-15");
        assert_eq!(body["end"]["date"], "2026-08-16");
        assert!(body["start"].get("timeZone").is_none());
    }

    #[test]
    fn create_body_includes_only_valid_deterministic_event_ids() {
        let provider_event_id = "mnema0123456789abcdef";
        let body = google_event_create_body(&timed_draft(Some(provider_event_id))).unwrap();
        assert_eq!(body["id"], provider_event_id);

        let body = google_event_create_body(&timed_draft(None)).unwrap();
        assert!(body.get("id").is_none());

        for invalid in [
            "abcd".to_string(),
            "abcw0".to_string(),
            "ABCDEF".to_string(),
            "abc-12".to_string(),
            "a".repeat(1025),
        ] {
            assert!(matches!(
                google_event_create_body(&timed_draft(Some(&invalid))),
                Err(CalendarError::InvalidResponse(_))
            ));
        }
    }

    #[test]
    fn update_body_never_serializes_the_draft_event_id() {
        let body = google_event_body(&timed_draft(Some("mnema0123456789abcdef"))).unwrap();
        assert!(body.get("id").is_none());
    }
}
