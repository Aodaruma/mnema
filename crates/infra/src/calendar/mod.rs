mod credentials;
mod encrypted_credentials;
mod google;
mod oauth;

use mnema_core::prelude::*;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::OffsetDateTime;

pub use credentials::{
    CredentialStore, CredentialStoreError, KeyringCredentialStore, MemoryCredentialStore,
    OAuthCredential,
};
pub use encrypted_credentials::EncryptedFileCredentialStore;
pub use google::{GoogleCalendarAdapter, GoogleCalendarConfig};
pub use oauth::OAuthPkce;

#[derive(Debug, Error)]
pub enum CalendarError {
    #[error(transparent)]
    Credential(#[from] CredentialStoreError),
    #[error("calendar HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("calendar URL error: {0}")]
    Url(String),
    #[error("calendar provider returned HTTP {status}: {body}")]
    Provider { status: u16, body: String },
    #[error("calendar authorization is missing for account")]
    MissingCredential,
    #[error("calendar refresh token is missing; authorization is required again")]
    MissingRefreshToken,
    #[error("calendar authorization was rejected; authorization is required again")]
    Unauthorized,
    #[error("calendar sync token expired; a full sync is required")]
    SyncTokenExpired,
    #[error("calendar full sync is required before incremental sync")]
    FullSyncRequired,
    #[error("calendar account is read-only")]
    WriteNotAllowed,
    #[error("unsupported calendar provider")]
    UnsupportedProvider,
    #[error("invalid calendar time range")]
    InvalidRange,
    #[error("invalid calendar provider response: {0}")]
    InvalidResponse(String),
    #[error("calendar configuration is missing: {0}")]
    MissingConfiguration(String),
    #[error("calendar repository error: {0}")]
    Repository(String),
}

impl From<CoreError> for CalendarError {
    fn from(value: CoreError) -> Self {
        Self::Repository(value.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteCalendar {
    pub id: String,
    pub summary: String,
    pub primary: bool,
    pub selected: bool,
    pub access_role: Option<String>,
    pub timezone: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalendarEventListMode {
    Full {
        time_min: Option<OffsetDateTime>,
        time_max: Option<OffsetDateTime>,
    },
    Incremental {
        sync_token: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarEventListRequest {
    pub calendar_id: String,
    pub mode: CalendarEventListMode,
    pub page_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarEventTombstone {
    pub provider_event_id: String,
    pub etag: Option<String>,
    pub provider_updated_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalendarEventChange {
    Upsert(Box<ExternalEvent>),
    Cancelled(CalendarEventTombstone),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarEventPage {
    pub changes: Vec<CalendarEventChange>,
    pub next_page_token: Option<String>,
    pub next_sync_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalendarEventDraft {
    /// Optional provider-side identity used to make create retries idempotent.
    /// Provider adapters validate their own identifier constraints. Updates use
    /// the event ID from the request path and never serialize this field.
    #[serde(default)]
    pub provider_event_id: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub location: Option<String>,
    pub start_at: OffsetDateTime,
    pub end_at: OffsetDateTime,
    pub all_day: bool,
    pub timezone: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CalendarSyncReport {
    pub upserted: usize,
    pub cancelled: usize,
    pub pages: usize,
    pub next_sync_token: Option<String>,
}

#[async_trait::async_trait]
pub trait CalendarReadApi: Send + Sync {
    async fn list_calendars(
        &self,
        account: &CalendarAccount,
    ) -> Result<Vec<RemoteCalendar>, CalendarError>;
    async fn list_event_page(
        &self,
        account: &CalendarAccount,
        request: CalendarEventListRequest,
    ) -> Result<CalendarEventPage, CalendarError>;
}

#[async_trait::async_trait]
pub trait CalendarWriteApi: Send + Sync {
    /// Reuses `account.managed_calendar_id` when configured; otherwise creates
    /// a dedicated provider calendar and returns its remote identity. Persisting
    /// the returned ID on the account remains the caller's responsibility.
    async fn ensure_managed_calendar(
        &self,
        account: &CalendarAccount,
        summary: &str,
    ) -> Result<RemoteCalendar, CalendarError>;
    async fn create_event(
        &self,
        account: &CalendarAccount,
        calendar_id: &str,
        draft: CalendarEventDraft,
    ) -> Result<ExternalEvent, CalendarError>;
    async fn update_event(
        &self,
        account: &CalendarAccount,
        calendar_id: &str,
        provider_event_id: &str,
        expected_etag: Option<&str>,
        draft: CalendarEventDraft,
    ) -> Result<ExternalEvent, CalendarError>;
    async fn delete_event(
        &self,
        account: &CalendarAccount,
        calendar_id: &str,
        provider_event_id: &str,
        expected_etag: Option<&str>,
    ) -> Result<(), CalendarError>;
}
