use crate::ids::{
    CalendarAccountId, ExternalEventId, ManagedCalendarEventId, ScheduleBlockId, UserId,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CalendarProvider {
    Google,
    Other(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CalendarAccessMode {
    ReadOnly,
    ReadWrite,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalendarAccount {
    pub id: CalendarAccountId,
    pub user_id: UserId,
    pub provider: CalendarProvider,
    pub provider_account_id: String,
    pub display_name: String,
    pub email: Option<String>,
    pub access_mode: CalendarAccessMode,
    pub enabled: bool,
    /// Provider calendar IDs whose events should constrain planning.
    pub selected_calendar_ids: Vec<String>,
    /// Provider calendar ID used for Mnema-managed write-back events.
    pub managed_calendar_id: Option<String>,
    /// IANA timezone name reported or selected for the account.
    pub timezone: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExternalEventStatus {
    Confirmed,
    Tentative,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExternalEventTransparency {
    Opaque,
    Transparent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalEvent {
    pub id: ExternalEventId,
    pub account_id: CalendarAccountId,
    pub calendar_id: String,
    pub provider_event_id: String,
    pub recurring_event_id: Option<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub original_start_at: Option<OffsetDateTime>,
    pub title: String,
    pub description: Option<String>,
    pub location: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub start_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub end_at: OffsetDateTime,
    pub all_day: bool,
    pub timezone: Option<String>,
    pub status: ExternalEventStatus,
    pub transparency: ExternalEventTransparency,
    /// Local override. `None` means use the scheduling preference/default.
    pub needs_travel: Option<bool>,
    /// Local override retained across provider sync when the incoming value is `None`.
    pub travel_before_minutes: Option<u32>,
    /// Local override retained across provider sync when the incoming value is `None`.
    pub travel_after_minutes: Option<u32>,
    pub etag: Option<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub provider_updated_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

impl ExternalEvent {
    #[must_use]
    pub fn blocks_time(&self) -> bool {
        self.status != ExternalEventStatus::Cancelled
            && self.transparency == ExternalEventTransparency::Opaque
    }

    #[must_use]
    pub fn overlaps(&self, start: OffsetDateTime, end: OffsetDateTime) -> bool {
        self.start_at < end && self.end_at > start
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalendarSyncCursor {
    pub account_id: CalendarAccountId,
    pub calendar_id: String,
    pub sync_token: Option<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_full_sync_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_incremental_sync_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedCalendarEventLink {
    pub id: ManagedCalendarEventId,
    pub account_id: CalendarAccountId,
    pub schedule_block_id: ScheduleBlockId,
    pub calendar_id: String,
    pub provider_event_id: String,
    pub etag: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}
