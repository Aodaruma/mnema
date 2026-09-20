use anyhow::Result;
use mnema_core::prelude::*;
use sqlx::{PgPool, Row, SqlitePool};
use time::format_description::well_known::Rfc3339;
use time::{Date, OffsetDateTime};

use super::sqlite_time::to_sqlite_timestamp;

fn map_storage_err(error: impl ToString) -> CoreError {
    CoreError::Storage(error.to_string())
}

fn u32_to_i32(value: u32, field: &str) -> CoreResult<i32> {
    i32::try_from(value)
        .map_err(|_| CoreError::Storage(format!("{field} exceeds PostgreSQL INTEGER range")))
}

fn optional_u32_to_i32(value: Option<u32>, field: &str) -> CoreResult<Option<i32>> {
    value.map(|value| u32_to_i32(value, field)).transpose()
}

fn to_rfc3339(value: OffsetDateTime) -> Result<String> {
    to_sqlite_timestamp(value)
}

fn from_rfc3339(value: &str) -> Result<OffsetDateTime> {
    OffsetDateTime::parse(value, &Rfc3339).map_err(Into::into)
}

fn optional_to_rfc3339(value: Option<OffsetDateTime>) -> Result<Option<String>> {
    value.map(to_rfc3339).transpose()
}

fn optional_from_rfc3339(value: Option<String>) -> Result<Option<OffsetDateTime>> {
    value.as_deref().map(from_rfc3339).transpose()
}

fn date_to_string(value: Date) -> String {
    value.to_string()
}

fn date_from_string(value: &str) -> Result<Date> {
    Date::parse(
        value,
        time::macros::format_description!("[year]-[month]-[day]"),
    )
    .map_err(Into::into)
}

fn calendar_provider_to_string(provider: &CalendarProvider) -> String {
    match provider {
        CalendarProvider::Google => "GOOGLE".into(),
        CalendarProvider::Other(value) => format!("OTHER:{value}"),
    }
}

fn calendar_provider_from_string(value: &str) -> CalendarProvider {
    match value {
        "GOOGLE" => CalendarProvider::Google,
        value if value.starts_with("OTHER:") => {
            CalendarProvider::Other(value.trim_start_matches("OTHER:").to_string())
        }
        value => CalendarProvider::Other(value.to_string()),
    }
}

fn access_mode_to_str(mode: CalendarAccessMode) -> &'static str {
    match mode {
        CalendarAccessMode::ReadOnly => "READ_ONLY",
        CalendarAccessMode::ReadWrite => "READ_WRITE",
    }
}

fn access_mode_from_str(value: &str) -> CalendarAccessMode {
    match value {
        "READ_WRITE" => CalendarAccessMode::ReadWrite,
        _ => CalendarAccessMode::ReadOnly,
    }
}

fn event_status_to_str(status: ExternalEventStatus) -> &'static str {
    match status {
        ExternalEventStatus::Confirmed => "CONFIRMED",
        ExternalEventStatus::Tentative => "TENTATIVE",
        ExternalEventStatus::Cancelled => "CANCELLED",
    }
}

fn event_status_from_str(value: &str) -> ExternalEventStatus {
    match value {
        "TENTATIVE" => ExternalEventStatus::Tentative,
        "CANCELLED" => ExternalEventStatus::Cancelled,
        _ => ExternalEventStatus::Confirmed,
    }
}

fn transparency_to_str(value: ExternalEventTransparency) -> &'static str {
    match value {
        ExternalEventTransparency::Opaque => "OPAQUE",
        ExternalEventTransparency::Transparent => "TRANSPARENT",
    }
}

fn transparency_from_str(value: &str) -> ExternalEventTransparency {
    match value {
        "TRANSPARENT" => ExternalEventTransparency::Transparent,
        _ => ExternalEventTransparency::Opaque,
    }
}

fn flexibility_to_str(value: HabitFlexibility) -> &'static str {
    match value {
        HabitFlexibility::Required => "REQUIRED",
        HabitFlexibility::Flexible => "FLEXIBLE",
    }
}

fn flexibility_from_str(value: &str) -> HabitFlexibility {
    match value {
        "REQUIRED" => HabitFlexibility::Required,
        _ => HabitFlexibility::Flexible,
    }
}

fn occurrence_state_to_str(value: HabitOccurrenceState) -> &'static str {
    match value {
        HabitOccurrenceState::Pending => "PENDING",
        HabitOccurrenceState::Scheduled => "SCHEDULED",
        HabitOccurrenceState::Done => "DONE",
        HabitOccurrenceState::Skipped => "SKIPPED",
        HabitOccurrenceState::Snoozed => "SNOOZED",
    }
}

fn occurrence_state_from_str(value: &str) -> HabitOccurrenceState {
    match value {
        "SCHEDULED" => HabitOccurrenceState::Scheduled,
        "DONE" => HabitOccurrenceState::Done,
        "SKIPPED" => HabitOccurrenceState::Skipped,
        "SNOOZED" => HabitOccurrenceState::Snoozed,
        _ => HabitOccurrenceState::Pending,
    }
}

macro_rules! repository_type {
    ($name:ident, $pool:ty) => {
        #[derive(Clone)]
        pub struct $name {
            pool: $pool,
        }

        impl $name {
            #[must_use]
            pub fn new(pool: $pool) -> Self {
                Self { pool }
            }
        }
    };
}

repository_type!(PostgresCalendarAccountRepository, PgPool);
repository_type!(SqliteCalendarAccountRepository, SqlitePool);
repository_type!(PostgresExternalEventRepository, PgPool);
repository_type!(SqliteExternalEventRepository, SqlitePool);
repository_type!(PostgresCalendarSyncCursorRepository, PgPool);
repository_type!(SqliteCalendarSyncCursorRepository, SqlitePool);
repository_type!(PostgresManagedCalendarEventLinkRepository, PgPool);
repository_type!(SqliteManagedCalendarEventLinkRepository, SqlitePool);
repository_type!(PostgresHabitRepository, PgPool);
repository_type!(SqliteHabitRepository, SqlitePool);
repository_type!(PostgresHabitOccurrenceRepository, PgPool);
repository_type!(SqliteHabitOccurrenceRepository, SqlitePool);
repository_type!(PostgresSchedulingPreferencesRepository, PgPool);
repository_type!(SqliteSchedulingPreferencesRepository, SqlitePool);

#[async_trait::async_trait]
impl CalendarAccountRepository for PostgresCalendarAccountRepository {
    async fn upsert(&self, account: CalendarAccount) -> CoreResult<()> {
        let selected_calendar_ids =
            serde_json::to_value(&account.selected_calendar_ids).map_err(map_storage_err)?;
        sqlx::query(
            r#"
            INSERT INTO calendar_accounts (
                id, user_id, provider, provider_account_id, display_name, email,
                access_mode, enabled, selected_calendar_ids, managed_calendar_id,
                timezone, created_at, updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
            ON CONFLICT(provider, provider_account_id) DO UPDATE SET
                user_id = excluded.user_id,
                display_name = excluded.display_name,
                email = excluded.email,
                access_mode = excluded.access_mode,
                enabled = excluded.enabled,
                selected_calendar_ids = excluded.selected_calendar_ids,
                managed_calendar_id = excluded.managed_calendar_id,
                timezone = excluded.timezone,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(account.id.0)
        .bind(account.user_id.0)
        .bind(calendar_provider_to_string(&account.provider))
        .bind(account.provider_account_id)
        .bind(account.display_name)
        .bind(account.email)
        .bind(access_mode_to_str(account.access_mode))
        .bind(account.enabled)
        .bind(selected_calendar_ids)
        .bind(account.managed_calendar_id)
        .bind(account.timezone)
        .bind(account.created_at)
        .bind(account.updated_at)
        .execute(&self.pool)
        .await
        .map_err(map_storage_err)?;
        Ok(())
    }

    async fn find(&self, id: CalendarAccountId) -> CoreResult<Option<CalendarAccount>> {
        sqlx::query("SELECT * FROM calendar_accounts WHERE id = $1")
            .bind(id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(map_storage_err)?
            .map(row_to_calendar_account_postgres)
            .transpose()
            .map_err(map_storage_err)
    }

    async fn find_by_provider_identity(
        &self,
        provider: CalendarProvider,
        provider_account_id: String,
    ) -> CoreResult<Option<CalendarAccount>> {
        sqlx::query(
            "SELECT * FROM calendar_accounts WHERE provider = $1 AND provider_account_id = $2",
        )
        .bind(calendar_provider_to_string(&provider))
        .bind(provider_account_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_storage_err)?
        .map(row_to_calendar_account_postgres)
        .transpose()
        .map_err(map_storage_err)
    }

    async fn list_enabled(&self, user_id: UserId) -> CoreResult<Vec<CalendarAccount>> {
        sqlx::query(
            "SELECT * FROM calendar_accounts WHERE user_id = $1 AND enabled = TRUE ORDER BY display_name, id",
        )
        .bind(user_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(map_storage_err)?
        .into_iter()
        .map(row_to_calendar_account_postgres)
        .map(|result| result.map_err(map_storage_err))
        .collect()
    }
}

fn row_to_calendar_account_postgres(row: sqlx::postgres::PgRow) -> Result<CalendarAccount> {
    let selected_calendar_ids: serde_json::Value = row.try_get("selected_calendar_ids")?;
    Ok(CalendarAccount {
        id: CalendarAccountId::from(row.try_get::<uuid::Uuid, _>("id")?),
        user_id: UserId::from(row.try_get::<uuid::Uuid, _>("user_id")?),
        provider: calendar_provider_from_string(&row.try_get::<String, _>("provider")?),
        provider_account_id: row.try_get("provider_account_id")?,
        display_name: row.try_get("display_name")?,
        email: row.try_get("email")?,
        access_mode: access_mode_from_str(&row.try_get::<String, _>("access_mode")?),
        enabled: row.try_get("enabled")?,
        selected_calendar_ids: serde_json::from_value(selected_calendar_ids)?,
        managed_calendar_id: row.try_get("managed_calendar_id")?,
        timezone: row.try_get("timezone")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

#[async_trait::async_trait]
impl CalendarAccountRepository for SqliteCalendarAccountRepository {
    async fn upsert(&self, account: CalendarAccount) -> CoreResult<()> {
        let selected_calendar_ids =
            serde_json::to_string(&account.selected_calendar_ids).map_err(map_storage_err)?;
        sqlx::query(
            r#"
            INSERT INTO calendar_accounts (
                id, user_id, provider, provider_account_id, display_name, email,
                access_mode, enabled, selected_calendar_ids, managed_calendar_id,
                timezone, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(provider, provider_account_id) DO UPDATE SET
                user_id = excluded.user_id,
                display_name = excluded.display_name,
                email = excluded.email,
                access_mode = excluded.access_mode,
                enabled = excluded.enabled,
                selected_calendar_ids = excluded.selected_calendar_ids,
                managed_calendar_id = excluded.managed_calendar_id,
                timezone = excluded.timezone,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(account.id.0.to_string())
        .bind(account.user_id.0.to_string())
        .bind(calendar_provider_to_string(&account.provider))
        .bind(account.provider_account_id)
        .bind(account.display_name)
        .bind(account.email)
        .bind(access_mode_to_str(account.access_mode))
        .bind(if account.enabled { 1 } else { 0 })
        .bind(selected_calendar_ids)
        .bind(account.managed_calendar_id)
        .bind(account.timezone)
        .bind(to_rfc3339(account.created_at).map_err(map_storage_err)?)
        .bind(to_rfc3339(account.updated_at).map_err(map_storage_err)?)
        .execute(&self.pool)
        .await
        .map_err(map_storage_err)?;
        Ok(())
    }

    async fn find(&self, id: CalendarAccountId) -> CoreResult<Option<CalendarAccount>> {
        sqlx::query("SELECT * FROM calendar_accounts WHERE id = ?")
            .bind(id.0.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(map_storage_err)?
            .map(row_to_calendar_account_sqlite)
            .transpose()
            .map_err(map_storage_err)
    }

    async fn find_by_provider_identity(
        &self,
        provider: CalendarProvider,
        provider_account_id: String,
    ) -> CoreResult<Option<CalendarAccount>> {
        sqlx::query(
            "SELECT * FROM calendar_accounts WHERE provider = ? AND provider_account_id = ?",
        )
        .bind(calendar_provider_to_string(&provider))
        .bind(provider_account_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_storage_err)?
        .map(row_to_calendar_account_sqlite)
        .transpose()
        .map_err(map_storage_err)
    }

    async fn list_enabled(&self, user_id: UserId) -> CoreResult<Vec<CalendarAccount>> {
        sqlx::query(
            "SELECT * FROM calendar_accounts WHERE user_id = ? AND enabled = 1 ORDER BY display_name, id",
        )
        .bind(user_id.0.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_storage_err)?
        .into_iter()
        .map(row_to_calendar_account_sqlite)
        .map(|result| result.map_err(map_storage_err))
        .collect()
    }
}

fn row_to_calendar_account_sqlite(row: sqlx::sqlite::SqliteRow) -> Result<CalendarAccount> {
    Ok(CalendarAccount {
        id: CalendarAccountId::from(uuid::Uuid::parse_str(&row.try_get::<String, _>("id")?)?),
        user_id: UserId::from(uuid::Uuid::parse_str(
            &row.try_get::<String, _>("user_id")?,
        )?),
        provider: calendar_provider_from_string(&row.try_get::<String, _>("provider")?),
        provider_account_id: row.try_get("provider_account_id")?,
        display_name: row.try_get("display_name")?,
        email: row.try_get("email")?,
        access_mode: access_mode_from_str(&row.try_get::<String, _>("access_mode")?),
        enabled: row.try_get::<i64, _>("enabled")? != 0,
        selected_calendar_ids: serde_json::from_str(
            &row.try_get::<String, _>("selected_calendar_ids")?,
        )?,
        managed_calendar_id: row.try_get("managed_calendar_id")?,
        timezone: row.try_get("timezone")?,
        created_at: from_rfc3339(&row.try_get::<String, _>("created_at")?)?,
        updated_at: from_rfc3339(&row.try_get::<String, _>("updated_at")?)?,
    })
}

fn validate_event_window(event: &ExternalEvent) -> CoreResult<()> {
    if event.end_at <= event.start_at {
        return Err(CoreError::Storage(
            "external event end must be after start".into(),
        ));
    }
    Ok(())
}

#[async_trait::async_trait]
impl ExternalEventRepository for PostgresExternalEventRepository {
    async fn upsert(&self, event: ExternalEvent) -> CoreResult<()> {
        validate_event_window(&event)?;
        let travel_before_minutes =
            optional_u32_to_i32(event.travel_before_minutes, "travel_before_minutes")?;
        let travel_after_minutes =
            optional_u32_to_i32(event.travel_after_minutes, "travel_after_minutes")?;
        sqlx::query(
            r#"
            INSERT INTO external_events (
                id, account_id, calendar_id, provider_event_id, recurring_event_id,
                original_start_at, title, description, location, start_at, end_at,
                all_day, timezone, status, transparency, needs_travel,
                travel_before_minutes, travel_after_minutes, etag, provider_updated_at,
                created_at, updated_at
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11,
                $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22
            )
            ON CONFLICT(account_id, calendar_id, provider_event_id) DO UPDATE SET
                recurring_event_id = excluded.recurring_event_id,
                original_start_at = excluded.original_start_at,
                title = excluded.title,
                description = excluded.description,
                location = excluded.location,
                start_at = excluded.start_at,
                end_at = excluded.end_at,
                all_day = excluded.all_day,
                timezone = excluded.timezone,
                status = excluded.status,
                transparency = excluded.transparency,
                needs_travel = COALESCE(excluded.needs_travel, external_events.needs_travel),
                travel_before_minutes = COALESCE(
                    excluded.travel_before_minutes,
                    external_events.travel_before_minutes
                ),
                travel_after_minutes = COALESCE(
                    excluded.travel_after_minutes,
                    external_events.travel_after_minutes
                ),
                etag = excluded.etag,
                provider_updated_at = excluded.provider_updated_at,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(event.id.0)
        .bind(event.account_id.0)
        .bind(event.calendar_id)
        .bind(event.provider_event_id)
        .bind(event.recurring_event_id)
        .bind(event.original_start_at)
        .bind(event.title)
        .bind(event.description)
        .bind(event.location)
        .bind(event.start_at)
        .bind(event.end_at)
        .bind(event.all_day)
        .bind(event.timezone)
        .bind(event_status_to_str(event.status))
        .bind(transparency_to_str(event.transparency))
        .bind(event.needs_travel)
        .bind(travel_before_minutes)
        .bind(travel_after_minutes)
        .bind(event.etag)
        .bind(event.provider_updated_at)
        .bind(event.created_at)
        .bind(event.updated_at)
        .execute(&self.pool)
        .await
        .map_err(map_storage_err)?;
        Ok(())
    }

    async fn find(&self, id: ExternalEventId) -> CoreResult<Option<ExternalEvent>> {
        sqlx::query("SELECT * FROM external_events WHERE id = $1")
            .bind(id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(map_storage_err)?
            .map(row_to_external_event_postgres)
            .transpose()
            .map_err(map_storage_err)
    }

    async fn find_by_provider_event(
        &self,
        account_id: CalendarAccountId,
        calendar_id: String,
        provider_event_id: String,
    ) -> CoreResult<Option<ExternalEvent>> {
        sqlx::query(
            "SELECT * FROM external_events WHERE account_id = $1 AND calendar_id = $2 AND provider_event_id = $3",
        )
        .bind(account_id.0)
        .bind(calendar_id)
        .bind(provider_event_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_storage_err)?
        .map(row_to_external_event_postgres)
        .transpose()
        .map_err(map_storage_err)
    }

    async fn list_overlapping(
        &self,
        start: OffsetDateTime,
        end: OffsetDateTime,
    ) -> CoreResult<Vec<ExternalEvent>> {
        list_external_events_postgres(&self.pool, None, start, end).await
    }

    async fn list_overlapping_for_account(
        &self,
        account_id: CalendarAccountId,
        start: OffsetDateTime,
        end: OffsetDateTime,
    ) -> CoreResult<Vec<ExternalEvent>> {
        list_external_events_postgres(&self.pool, Some(account_id), start, end).await
    }

    async fn delete_unseen_for_calendar(
        &self,
        account_id: CalendarAccountId,
        calendar_id: String,
        retained_provider_event_ids: Vec<String>,
    ) -> CoreResult<usize> {
        let retained = retained_provider_event_ids
            .into_iter()
            .collect::<std::collections::HashSet<_>>();
        let mut tx = self.pool.begin().await.map_err(map_storage_err)?;
        let rows = sqlx::query(
            "SELECT id, provider_event_id FROM external_events WHERE account_id = $1 AND calendar_id = $2",
        )
        .bind(account_id.0)
        .bind(calendar_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(map_storage_err)?;
        let mut deleted = 0;
        for row in rows {
            let provider_event_id = row
                .try_get::<String, _>("provider_event_id")
                .map_err(map_storage_err)?;
            if !retained.contains(&provider_event_id) {
                sqlx::query("DELETE FROM external_events WHERE id = $1")
                    .bind(
                        row.try_get::<uuid::Uuid, _>("id")
                            .map_err(map_storage_err)?,
                    )
                    .execute(&mut *tx)
                    .await
                    .map_err(map_storage_err)?;
                deleted += 1;
            }
        }
        tx.commit().await.map_err(map_storage_err)?;
        Ok(deleted)
    }
}

async fn list_external_events_postgres(
    pool: &PgPool,
    account_id: Option<CalendarAccountId>,
    start: OffsetDateTime,
    end: OffsetDateTime,
) -> CoreResult<Vec<ExternalEvent>> {
    if start >= end {
        return Err(CoreError::Storage("invalid overlap range".into()));
    }
    let rows = if let Some(account_id) = account_id {
        sqlx::query(
            r#"
            SELECT external_events.*
            FROM external_events
            INNER JOIN calendar_accounts
                ON calendar_accounts.id = external_events.account_id
            WHERE external_events.account_id = $1
              AND calendar_accounts.enabled = true
              AND calendar_accounts.selected_calendar_ids ? external_events.calendar_id
              AND external_events.start_at < $2
              AND external_events.end_at > $3
            ORDER BY external_events.start_at, external_events.end_at, external_events.id
            "#,
        )
        .bind(account_id.0)
        .bind(end)
        .bind(start)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query(
            r#"
            SELECT external_events.*
            FROM external_events
            INNER JOIN calendar_accounts
                ON calendar_accounts.id = external_events.account_id
            WHERE calendar_accounts.enabled = true
              AND calendar_accounts.selected_calendar_ids ? external_events.calendar_id
              AND external_events.start_at < $1
              AND external_events.end_at > $2
            ORDER BY external_events.start_at, external_events.end_at, external_events.id
            "#,
        )
        .bind(end)
        .bind(start)
        .fetch_all(pool)
        .await
    }
    .map_err(map_storage_err)?;

    rows.into_iter()
        .map(row_to_external_event_postgres)
        .map(|result| result.map_err(map_storage_err))
        .collect()
}

fn row_to_external_event_postgres(row: sqlx::postgres::PgRow) -> Result<ExternalEvent> {
    Ok(ExternalEvent {
        id: ExternalEventId::from(row.try_get::<uuid::Uuid, _>("id")?),
        account_id: CalendarAccountId::from(row.try_get::<uuid::Uuid, _>("account_id")?),
        calendar_id: row.try_get("calendar_id")?,
        provider_event_id: row.try_get("provider_event_id")?,
        recurring_event_id: row.try_get("recurring_event_id")?,
        original_start_at: row.try_get("original_start_at")?,
        title: row.try_get("title")?,
        description: row.try_get("description")?,
        location: row.try_get("location")?,
        start_at: row.try_get("start_at")?,
        end_at: row.try_get("end_at")?,
        all_day: row.try_get("all_day")?,
        timezone: row.try_get("timezone")?,
        status: event_status_from_str(&row.try_get::<String, _>("status")?),
        transparency: transparency_from_str(&row.try_get::<String, _>("transparency")?),
        needs_travel: row.try_get("needs_travel")?,
        travel_before_minutes: row
            .try_get::<Option<i32>, _>("travel_before_minutes")?
            .map(u32::try_from)
            .transpose()?,
        travel_after_minutes: row
            .try_get::<Option<i32>, _>("travel_after_minutes")?
            .map(u32::try_from)
            .transpose()?,
        etag: row.try_get("etag")?,
        provider_updated_at: row.try_get("provider_updated_at")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

#[async_trait::async_trait]
impl ExternalEventRepository for SqliteExternalEventRepository {
    async fn upsert(&self, event: ExternalEvent) -> CoreResult<()> {
        validate_event_window(&event)?;
        sqlx::query(
            r#"
            INSERT INTO external_events (
                id, account_id, calendar_id, provider_event_id, recurring_event_id,
                original_start_at, title, description, location, start_at, end_at,
                all_day, timezone, status, transparency, needs_travel,
                travel_before_minutes, travel_after_minutes, etag, provider_updated_at,
                created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(account_id, calendar_id, provider_event_id) DO UPDATE SET
                recurring_event_id = excluded.recurring_event_id,
                original_start_at = excluded.original_start_at,
                title = excluded.title,
                description = excluded.description,
                location = excluded.location,
                start_at = excluded.start_at,
                end_at = excluded.end_at,
                all_day = excluded.all_day,
                timezone = excluded.timezone,
                status = excluded.status,
                transparency = excluded.transparency,
                needs_travel = COALESCE(excluded.needs_travel, external_events.needs_travel),
                travel_before_minutes = COALESCE(
                    excluded.travel_before_minutes,
                    external_events.travel_before_minutes
                ),
                travel_after_minutes = COALESCE(
                    excluded.travel_after_minutes,
                    external_events.travel_after_minutes
                ),
                etag = excluded.etag,
                provider_updated_at = excluded.provider_updated_at,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(event.id.0.to_string())
        .bind(event.account_id.0.to_string())
        .bind(event.calendar_id)
        .bind(event.provider_event_id)
        .bind(event.recurring_event_id)
        .bind(optional_to_rfc3339(event.original_start_at).map_err(map_storage_err)?)
        .bind(event.title)
        .bind(event.description)
        .bind(event.location)
        .bind(to_rfc3339(event.start_at).map_err(map_storage_err)?)
        .bind(to_rfc3339(event.end_at).map_err(map_storage_err)?)
        .bind(if event.all_day { 1 } else { 0 })
        .bind(event.timezone)
        .bind(event_status_to_str(event.status))
        .bind(transparency_to_str(event.transparency))
        .bind(event.needs_travel.map(|value| if value { 1 } else { 0 }))
        .bind(event.travel_before_minutes.map(i64::from))
        .bind(event.travel_after_minutes.map(i64::from))
        .bind(event.etag)
        .bind(optional_to_rfc3339(event.provider_updated_at).map_err(map_storage_err)?)
        .bind(to_rfc3339(event.created_at).map_err(map_storage_err)?)
        .bind(to_rfc3339(event.updated_at).map_err(map_storage_err)?)
        .execute(&self.pool)
        .await
        .map_err(map_storage_err)?;
        Ok(())
    }

    async fn find(&self, id: ExternalEventId) -> CoreResult<Option<ExternalEvent>> {
        sqlx::query("SELECT * FROM external_events WHERE id = ?")
            .bind(id.0.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(map_storage_err)?
            .map(row_to_external_event_sqlite)
            .transpose()
            .map_err(map_storage_err)
    }

    async fn find_by_provider_event(
        &self,
        account_id: CalendarAccountId,
        calendar_id: String,
        provider_event_id: String,
    ) -> CoreResult<Option<ExternalEvent>> {
        sqlx::query(
            "SELECT * FROM external_events WHERE account_id = ? AND calendar_id = ? AND provider_event_id = ?",
        )
        .bind(account_id.0.to_string())
        .bind(calendar_id)
        .bind(provider_event_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_storage_err)?
        .map(row_to_external_event_sqlite)
        .transpose()
        .map_err(map_storage_err)
    }

    async fn list_overlapping(
        &self,
        start: OffsetDateTime,
        end: OffsetDateTime,
    ) -> CoreResult<Vec<ExternalEvent>> {
        list_external_events_sqlite(&self.pool, None, start, end).await
    }

    async fn list_overlapping_for_account(
        &self,
        account_id: CalendarAccountId,
        start: OffsetDateTime,
        end: OffsetDateTime,
    ) -> CoreResult<Vec<ExternalEvent>> {
        list_external_events_sqlite(&self.pool, Some(account_id), start, end).await
    }

    async fn delete_unseen_for_calendar(
        &self,
        account_id: CalendarAccountId,
        calendar_id: String,
        retained_provider_event_ids: Vec<String>,
    ) -> CoreResult<usize> {
        let retained = retained_provider_event_ids
            .into_iter()
            .collect::<std::collections::HashSet<_>>();
        let mut tx = self.pool.begin().await.map_err(map_storage_err)?;
        let rows = sqlx::query(
            "SELECT id, provider_event_id FROM external_events WHERE account_id = ? AND calendar_id = ?",
        )
        .bind(account_id.0.to_string())
        .bind(calendar_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(map_storage_err)?;
        let mut deleted = 0;
        for row in rows {
            let provider_event_id = row
                .try_get::<String, _>("provider_event_id")
                .map_err(map_storage_err)?;
            if !retained.contains(&provider_event_id) {
                sqlx::query("DELETE FROM external_events WHERE id = ?")
                    .bind(row.try_get::<String, _>("id").map_err(map_storage_err)?)
                    .execute(&mut *tx)
                    .await
                    .map_err(map_storage_err)?;
                deleted += 1;
            }
        }
        tx.commit().await.map_err(map_storage_err)?;
        Ok(deleted)
    }
}

async fn list_external_events_sqlite(
    pool: &SqlitePool,
    account_id: Option<CalendarAccountId>,
    start: OffsetDateTime,
    end: OffsetDateTime,
) -> CoreResult<Vec<ExternalEvent>> {
    if start >= end {
        return Err(CoreError::Storage("invalid overlap range".into()));
    }
    let start = to_rfc3339(start).map_err(map_storage_err)?;
    let end = to_rfc3339(end).map_err(map_storage_err)?;
    let rows = if let Some(account_id) = account_id {
        sqlx::query(
            r#"
            SELECT external_events.*
            FROM external_events
            INNER JOIN calendar_accounts
                ON calendar_accounts.id = external_events.account_id
            WHERE external_events.account_id = ?
              AND calendar_accounts.enabled = 1
              AND EXISTS (
                  SELECT 1
                  FROM json_each(calendar_accounts.selected_calendar_ids) AS selected
                  WHERE selected.value = external_events.calendar_id
              )
              AND external_events.start_at < ?
              AND external_events.end_at > ?
            ORDER BY external_events.start_at, external_events.end_at, external_events.id
            "#,
        )
        .bind(account_id.0.to_string())
        .bind(&end)
        .bind(&start)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query(
            r#"
            SELECT external_events.*
            FROM external_events
            INNER JOIN calendar_accounts
                ON calendar_accounts.id = external_events.account_id
            WHERE calendar_accounts.enabled = 1
              AND EXISTS (
                  SELECT 1
                  FROM json_each(calendar_accounts.selected_calendar_ids) AS selected
                  WHERE selected.value = external_events.calendar_id
              )
              AND external_events.start_at < ?
              AND external_events.end_at > ?
            ORDER BY external_events.start_at, external_events.end_at, external_events.id
            "#,
        )
        .bind(&end)
        .bind(&start)
        .fetch_all(pool)
        .await
    }
    .map_err(map_storage_err)?;

    rows.into_iter()
        .map(row_to_external_event_sqlite)
        .map(|result| result.map_err(map_storage_err))
        .collect()
}

fn row_to_external_event_sqlite(row: sqlx::sqlite::SqliteRow) -> Result<ExternalEvent> {
    Ok(ExternalEvent {
        id: ExternalEventId::from(uuid::Uuid::parse_str(&row.try_get::<String, _>("id")?)?),
        account_id: CalendarAccountId::from(uuid::Uuid::parse_str(
            &row.try_get::<String, _>("account_id")?,
        )?),
        calendar_id: row.try_get("calendar_id")?,
        provider_event_id: row.try_get("provider_event_id")?,
        recurring_event_id: row.try_get("recurring_event_id")?,
        original_start_at: optional_from_rfc3339(row.try_get("original_start_at")?)?,
        title: row.try_get("title")?,
        description: row.try_get("description")?,
        location: row.try_get("location")?,
        start_at: from_rfc3339(&row.try_get::<String, _>("start_at")?)?,
        end_at: from_rfc3339(&row.try_get::<String, _>("end_at")?)?,
        all_day: row.try_get::<i64, _>("all_day")? != 0,
        timezone: row.try_get("timezone")?,
        status: event_status_from_str(&row.try_get::<String, _>("status")?),
        transparency: transparency_from_str(&row.try_get::<String, _>("transparency")?),
        needs_travel: row
            .try_get::<Option<i64>, _>("needs_travel")?
            .map(|value| value != 0),
        travel_before_minutes: row
            .try_get::<Option<i64>, _>("travel_before_minutes")?
            .map(u32::try_from)
            .transpose()?,
        travel_after_minutes: row
            .try_get::<Option<i64>, _>("travel_after_minutes")?
            .map(u32::try_from)
            .transpose()?,
        etag: row.try_get("etag")?,
        provider_updated_at: optional_from_rfc3339(row.try_get("provider_updated_at")?)?,
        created_at: from_rfc3339(&row.try_get::<String, _>("created_at")?)?,
        updated_at: from_rfc3339(&row.try_get::<String, _>("updated_at")?)?,
    })
}

#[async_trait::async_trait]
impl CalendarSyncCursorRepository for PostgresCalendarSyncCursorRepository {
    async fn upsert(&self, cursor: CalendarSyncCursor) -> CoreResult<()> {
        sqlx::query(
            r#"
            INSERT INTO calendar_sync_cursors (
                account_id, calendar_id, sync_token, last_full_sync_at,
                last_incremental_sync_at, updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT(account_id, calendar_id) DO UPDATE SET
                sync_token = excluded.sync_token,
                last_full_sync_at = excluded.last_full_sync_at,
                last_incremental_sync_at = excluded.last_incremental_sync_at,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(cursor.account_id.0)
        .bind(cursor.calendar_id)
        .bind(cursor.sync_token)
        .bind(cursor.last_full_sync_at)
        .bind(cursor.last_incremental_sync_at)
        .bind(cursor.updated_at)
        .execute(&self.pool)
        .await
        .map_err(map_storage_err)?;
        Ok(())
    }

    async fn get(
        &self,
        account_id: CalendarAccountId,
        calendar_id: String,
    ) -> CoreResult<Option<CalendarSyncCursor>> {
        sqlx::query(
            "SELECT * FROM calendar_sync_cursors WHERE account_id = $1 AND calendar_id = $2",
        )
        .bind(account_id.0)
        .bind(calendar_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_storage_err)?
        .map(row_to_sync_cursor_postgres)
        .transpose()
        .map_err(map_storage_err)
    }

    async fn clear(&self, account_id: CalendarAccountId, calendar_id: String) -> CoreResult<()> {
        sqlx::query("DELETE FROM calendar_sync_cursors WHERE account_id = $1 AND calendar_id = $2")
            .bind(account_id.0)
            .bind(calendar_id)
            .execute(&self.pool)
            .await
            .map_err(map_storage_err)?;
        Ok(())
    }
}

fn row_to_sync_cursor_postgres(row: sqlx::postgres::PgRow) -> Result<CalendarSyncCursor> {
    Ok(CalendarSyncCursor {
        account_id: CalendarAccountId::from(row.try_get::<uuid::Uuid, _>("account_id")?),
        calendar_id: row.try_get("calendar_id")?,
        sync_token: row.try_get("sync_token")?,
        last_full_sync_at: row.try_get("last_full_sync_at")?,
        last_incremental_sync_at: row.try_get("last_incremental_sync_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

#[async_trait::async_trait]
impl CalendarSyncCursorRepository for SqliteCalendarSyncCursorRepository {
    async fn upsert(&self, cursor: CalendarSyncCursor) -> CoreResult<()> {
        sqlx::query(
            r#"
            INSERT INTO calendar_sync_cursors (
                account_id, calendar_id, sync_token, last_full_sync_at,
                last_incremental_sync_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?)
            ON CONFLICT(account_id, calendar_id) DO UPDATE SET
                sync_token = excluded.sync_token,
                last_full_sync_at = excluded.last_full_sync_at,
                last_incremental_sync_at = excluded.last_incremental_sync_at,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(cursor.account_id.0.to_string())
        .bind(cursor.calendar_id)
        .bind(cursor.sync_token)
        .bind(optional_to_rfc3339(cursor.last_full_sync_at).map_err(map_storage_err)?)
        .bind(optional_to_rfc3339(cursor.last_incremental_sync_at).map_err(map_storage_err)?)
        .bind(to_rfc3339(cursor.updated_at).map_err(map_storage_err)?)
        .execute(&self.pool)
        .await
        .map_err(map_storage_err)?;
        Ok(())
    }

    async fn get(
        &self,
        account_id: CalendarAccountId,
        calendar_id: String,
    ) -> CoreResult<Option<CalendarSyncCursor>> {
        sqlx::query("SELECT * FROM calendar_sync_cursors WHERE account_id = ? AND calendar_id = ?")
            .bind(account_id.0.to_string())
            .bind(calendar_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(map_storage_err)?
            .map(row_to_sync_cursor_sqlite)
            .transpose()
            .map_err(map_storage_err)
    }

    async fn clear(&self, account_id: CalendarAccountId, calendar_id: String) -> CoreResult<()> {
        sqlx::query("DELETE FROM calendar_sync_cursors WHERE account_id = ? AND calendar_id = ?")
            .bind(account_id.0.to_string())
            .bind(calendar_id)
            .execute(&self.pool)
            .await
            .map_err(map_storage_err)?;
        Ok(())
    }
}

fn row_to_sync_cursor_sqlite(row: sqlx::sqlite::SqliteRow) -> Result<CalendarSyncCursor> {
    Ok(CalendarSyncCursor {
        account_id: CalendarAccountId::from(uuid::Uuid::parse_str(
            &row.try_get::<String, _>("account_id")?,
        )?),
        calendar_id: row.try_get("calendar_id")?,
        sync_token: row.try_get("sync_token")?,
        last_full_sync_at: optional_from_rfc3339(row.try_get("last_full_sync_at")?)?,
        last_incremental_sync_at: optional_from_rfc3339(row.try_get("last_incremental_sync_at")?)?,
        updated_at: from_rfc3339(&row.try_get::<String, _>("updated_at")?)?,
    })
}

#[async_trait::async_trait]
impl ManagedCalendarEventLinkRepository for PostgresManagedCalendarEventLinkRepository {
    async fn upsert(&self, link: ManagedCalendarEventLink) -> CoreResult<()> {
        sqlx::query(
            r#"
            INSERT INTO managed_calendar_event_links (
                id, account_id, schedule_block_id, calendar_id, provider_event_id,
                etag, created_at, updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT(account_id, schedule_block_id) DO UPDATE SET
                calendar_id = excluded.calendar_id,
                provider_event_id = excluded.provider_event_id,
                etag = excluded.etag,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(link.id.0)
        .bind(link.account_id.0)
        .bind(link.schedule_block_id.0)
        .bind(link.calendar_id)
        .bind(link.provider_event_id)
        .bind(link.etag)
        .bind(link.created_at)
        .bind(link.updated_at)
        .execute(&self.pool)
        .await
        .map_err(map_storage_err)?;
        Ok(())
    }

    async fn find(
        &self,
        id: ManagedCalendarEventId,
    ) -> CoreResult<Option<ManagedCalendarEventLink>> {
        sqlx::query("SELECT * FROM managed_calendar_event_links WHERE id = $1")
            .bind(id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(map_storage_err)?
            .map(row_to_managed_link_postgres)
            .transpose()
            .map_err(map_storage_err)
    }

    async fn find_by_schedule_block(
        &self,
        account_id: CalendarAccountId,
        schedule_block_id: ScheduleBlockId,
    ) -> CoreResult<Option<ManagedCalendarEventLink>> {
        sqlx::query(
            "SELECT * FROM managed_calendar_event_links WHERE account_id = $1 AND schedule_block_id = $2",
        )
        .bind(account_id.0)
        .bind(schedule_block_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_storage_err)?
        .map(row_to_managed_link_postgres)
        .transpose()
        .map_err(map_storage_err)
    }

    async fn find_by_provider_event(
        &self,
        account_id: CalendarAccountId,
        calendar_id: String,
        provider_event_id: String,
    ) -> CoreResult<Option<ManagedCalendarEventLink>> {
        sqlx::query(
            "SELECT * FROM managed_calendar_event_links WHERE account_id = $1 AND calendar_id = $2 AND provider_event_id = $3",
        )
        .bind(account_id.0)
        .bind(calendar_id)
        .bind(provider_event_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_storage_err)?
        .map(row_to_managed_link_postgres)
        .transpose()
        .map_err(map_storage_err)
    }

    async fn list_for_account(
        &self,
        account_id: CalendarAccountId,
    ) -> CoreResult<Vec<ManagedCalendarEventLink>> {
        let rows = sqlx::query(
            "SELECT * FROM managed_calendar_event_links WHERE account_id = $1 ORDER BY created_at, id",
        )
        .bind(account_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(map_storage_err)?;
        rows.into_iter()
            .map(row_to_managed_link_postgres)
            .map(|result| result.map_err(map_storage_err))
            .collect()
    }

    async fn delete(&self, id: ManagedCalendarEventId) -> CoreResult<()> {
        sqlx::query("DELETE FROM managed_calendar_event_links WHERE id = $1")
            .bind(id.0)
            .execute(&self.pool)
            .await
            .map_err(map_storage_err)?;
        Ok(())
    }
}

fn row_to_managed_link_postgres(row: sqlx::postgres::PgRow) -> Result<ManagedCalendarEventLink> {
    Ok(ManagedCalendarEventLink {
        id: ManagedCalendarEventId::from(row.try_get::<uuid::Uuid, _>("id")?),
        account_id: CalendarAccountId::from(row.try_get::<uuid::Uuid, _>("account_id")?),
        schedule_block_id: ScheduleBlockId::from(
            row.try_get::<uuid::Uuid, _>("schedule_block_id")?,
        ),
        calendar_id: row.try_get("calendar_id")?,
        provider_event_id: row.try_get("provider_event_id")?,
        etag: row.try_get("etag")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

#[async_trait::async_trait]
impl ManagedCalendarEventLinkRepository for SqliteManagedCalendarEventLinkRepository {
    async fn upsert(&self, link: ManagedCalendarEventLink) -> CoreResult<()> {
        sqlx::query(
            r#"
            INSERT INTO managed_calendar_event_links (
                id, account_id, schedule_block_id, calendar_id, provider_event_id,
                etag, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(account_id, schedule_block_id) DO UPDATE SET
                calendar_id = excluded.calendar_id,
                provider_event_id = excluded.provider_event_id,
                etag = excluded.etag,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(link.id.0.to_string())
        .bind(link.account_id.0.to_string())
        .bind(link.schedule_block_id.0.to_string())
        .bind(link.calendar_id)
        .bind(link.provider_event_id)
        .bind(link.etag)
        .bind(to_rfc3339(link.created_at).map_err(map_storage_err)?)
        .bind(to_rfc3339(link.updated_at).map_err(map_storage_err)?)
        .execute(&self.pool)
        .await
        .map_err(map_storage_err)?;
        Ok(())
    }

    async fn find(
        &self,
        id: ManagedCalendarEventId,
    ) -> CoreResult<Option<ManagedCalendarEventLink>> {
        sqlx::query("SELECT * FROM managed_calendar_event_links WHERE id = ?")
            .bind(id.0.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(map_storage_err)?
            .map(row_to_managed_link_sqlite)
            .transpose()
            .map_err(map_storage_err)
    }

    async fn find_by_schedule_block(
        &self,
        account_id: CalendarAccountId,
        schedule_block_id: ScheduleBlockId,
    ) -> CoreResult<Option<ManagedCalendarEventLink>> {
        sqlx::query(
            "SELECT * FROM managed_calendar_event_links WHERE account_id = ? AND schedule_block_id = ?",
        )
        .bind(account_id.0.to_string())
        .bind(schedule_block_id.0.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_storage_err)?
        .map(row_to_managed_link_sqlite)
        .transpose()
        .map_err(map_storage_err)
    }

    async fn find_by_provider_event(
        &self,
        account_id: CalendarAccountId,
        calendar_id: String,
        provider_event_id: String,
    ) -> CoreResult<Option<ManagedCalendarEventLink>> {
        sqlx::query(
            "SELECT * FROM managed_calendar_event_links WHERE account_id = ? AND calendar_id = ? AND provider_event_id = ?",
        )
        .bind(account_id.0.to_string())
        .bind(calendar_id)
        .bind(provider_event_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_storage_err)?
        .map(row_to_managed_link_sqlite)
        .transpose()
        .map_err(map_storage_err)
    }

    async fn list_for_account(
        &self,
        account_id: CalendarAccountId,
    ) -> CoreResult<Vec<ManagedCalendarEventLink>> {
        let rows = sqlx::query(
            "SELECT * FROM managed_calendar_event_links WHERE account_id = ? ORDER BY created_at, id",
        )
        .bind(account_id.0.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_storage_err)?;
        rows.into_iter()
            .map(row_to_managed_link_sqlite)
            .map(|result| result.map_err(map_storage_err))
            .collect()
    }

    async fn delete(&self, id: ManagedCalendarEventId) -> CoreResult<()> {
        sqlx::query("DELETE FROM managed_calendar_event_links WHERE id = ?")
            .bind(id.0.to_string())
            .execute(&self.pool)
            .await
            .map_err(map_storage_err)?;
        Ok(())
    }
}

fn row_to_managed_link_sqlite(row: sqlx::sqlite::SqliteRow) -> Result<ManagedCalendarEventLink> {
    Ok(ManagedCalendarEventLink {
        id: ManagedCalendarEventId::from(uuid::Uuid::parse_str(&row.try_get::<String, _>("id")?)?),
        account_id: CalendarAccountId::from(uuid::Uuid::parse_str(
            &row.try_get::<String, _>("account_id")?,
        )?),
        schedule_block_id: ScheduleBlockId::from(uuid::Uuid::parse_str(
            &row.try_get::<String, _>("schedule_block_id")?,
        )?),
        calendar_id: row.try_get("calendar_id")?,
        provider_event_id: row.try_get("provider_event_id")?,
        etag: row.try_get("etag")?,
        created_at: from_rfc3339(&row.try_get::<String, _>("created_at")?)?,
        updated_at: from_rfc3339(&row.try_get::<String, _>("updated_at")?)?,
    })
}

#[async_trait::async_trait]
impl HabitRepository for PostgresHabitRepository {
    async fn upsert(&self, habit: Habit) -> CoreResult<()> {
        habit.validate().map_err(map_storage_err)?;
        let schedule = serde_json::to_value(&habit.schedule).map_err(map_storage_err)?;
        let preferred_window = habit
            .preferred_window
            .map(serde_json::to_value)
            .transpose()
            .map_err(map_storage_err)?;
        sqlx::query(
            r#"
            INSERT INTO habits (
                id, user_id, title, schedule, duration_minutes, preferred_window,
                flexibility, enabled, disabled_at, created_at, updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            ON CONFLICT(id) DO UPDATE SET
                user_id = excluded.user_id,
                title = excluded.title,
                schedule = excluded.schedule,
                duration_minutes = excluded.duration_minutes,
                preferred_window = excluded.preferred_window,
                flexibility = excluded.flexibility,
                enabled = excluded.enabled,
                disabled_at = excluded.disabled_at,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(habit.id.0)
        .bind(habit.user_id.0)
        .bind(habit.title)
        .bind(schedule)
        .bind(u32_to_i32(habit.duration_minutes, "duration_minutes")?)
        .bind(preferred_window)
        .bind(flexibility_to_str(habit.flexibility))
        .bind(habit.enabled)
        .bind(habit.disabled_at)
        .bind(habit.created_at)
        .bind(habit.updated_at)
        .execute(&self.pool)
        .await
        .map_err(map_storage_err)?;
        Ok(())
    }

    async fn find(&self, id: HabitId) -> CoreResult<Option<Habit>> {
        sqlx::query("SELECT * FROM habits WHERE id = $1")
            .bind(id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(map_storage_err)?
            .map(row_to_habit_postgres)
            .transpose()
            .map_err(map_storage_err)
    }

    async fn list_enabled(&self, user_id: UserId) -> CoreResult<Vec<Habit>> {
        sqlx::query(
            "SELECT * FROM habits WHERE user_id = $1 AND enabled = TRUE AND disabled_at IS NULL ORDER BY title, id",
        )
        .bind(user_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(map_storage_err)?
        .into_iter()
        .map(row_to_habit_postgres)
        .map(|result| result.map_err(map_storage_err))
        .collect()
    }
}

fn row_to_habit_postgres(row: sqlx::postgres::PgRow) -> Result<Habit> {
    let schedule: serde_json::Value = row.try_get("schedule")?;
    let preferred_window: Option<serde_json::Value> = row.try_get("preferred_window")?;
    Ok(Habit {
        id: HabitId::from(row.try_get::<uuid::Uuid, _>("id")?),
        user_id: UserId::from(row.try_get::<uuid::Uuid, _>("user_id")?),
        title: row.try_get("title")?,
        schedule: serde_json::from_value(schedule)?,
        duration_minutes: u32::try_from(row.try_get::<i32, _>("duration_minutes")?)?,
        preferred_window: preferred_window.map(serde_json::from_value).transpose()?,
        flexibility: flexibility_from_str(&row.try_get::<String, _>("flexibility")?),
        enabled: row.try_get("enabled")?,
        disabled_at: row.try_get("disabled_at")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

#[async_trait::async_trait]
impl HabitRepository for SqliteHabitRepository {
    async fn upsert(&self, habit: Habit) -> CoreResult<()> {
        habit.validate().map_err(map_storage_err)?;
        let schedule = serde_json::to_string(&habit.schedule).map_err(map_storage_err)?;
        let preferred_window = habit
            .preferred_window
            .map(|window| serde_json::to_string(&window))
            .transpose()
            .map_err(map_storage_err)?;
        sqlx::query(
            r#"
            INSERT INTO habits (
                id, user_id, title, schedule, duration_minutes, preferred_window,
                flexibility, enabled, disabled_at, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                user_id = excluded.user_id,
                title = excluded.title,
                schedule = excluded.schedule,
                duration_minutes = excluded.duration_minutes,
                preferred_window = excluded.preferred_window,
                flexibility = excluded.flexibility,
                enabled = excluded.enabled,
                disabled_at = excluded.disabled_at,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(habit.id.0.to_string())
        .bind(habit.user_id.0.to_string())
        .bind(habit.title)
        .bind(schedule)
        .bind(i64::from(habit.duration_minutes))
        .bind(preferred_window)
        .bind(flexibility_to_str(habit.flexibility))
        .bind(if habit.enabled { 1 } else { 0 })
        .bind(optional_to_rfc3339(habit.disabled_at).map_err(map_storage_err)?)
        .bind(to_rfc3339(habit.created_at).map_err(map_storage_err)?)
        .bind(to_rfc3339(habit.updated_at).map_err(map_storage_err)?)
        .execute(&self.pool)
        .await
        .map_err(map_storage_err)?;
        Ok(())
    }

    async fn find(&self, id: HabitId) -> CoreResult<Option<Habit>> {
        sqlx::query("SELECT * FROM habits WHERE id = ?")
            .bind(id.0.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(map_storage_err)?
            .map(row_to_habit_sqlite)
            .transpose()
            .map_err(map_storage_err)
    }

    async fn list_enabled(&self, user_id: UserId) -> CoreResult<Vec<Habit>> {
        sqlx::query(
            "SELECT * FROM habits WHERE user_id = ? AND enabled = 1 AND disabled_at IS NULL ORDER BY title, id",
        )
        .bind(user_id.0.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_storage_err)?
        .into_iter()
        .map(row_to_habit_sqlite)
        .map(|result| result.map_err(map_storage_err))
        .collect()
    }
}

fn row_to_habit_sqlite(row: sqlx::sqlite::SqliteRow) -> Result<Habit> {
    let preferred_window: Option<String> = row.try_get("preferred_window")?;
    Ok(Habit {
        id: HabitId::from(uuid::Uuid::parse_str(&row.try_get::<String, _>("id")?)?),
        user_id: UserId::from(uuid::Uuid::parse_str(
            &row.try_get::<String, _>("user_id")?,
        )?),
        title: row.try_get("title")?,
        schedule: serde_json::from_str(&row.try_get::<String, _>("schedule")?)?,
        duration_minutes: u32::try_from(row.try_get::<i64, _>("duration_minutes")?)?,
        preferred_window: preferred_window
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?,
        flexibility: flexibility_from_str(&row.try_get::<String, _>("flexibility")?),
        enabled: row.try_get::<i64, _>("enabled")? != 0,
        disabled_at: optional_from_rfc3339(row.try_get("disabled_at")?)?,
        created_at: from_rfc3339(&row.try_get::<String, _>("created_at")?)?,
        updated_at: from_rfc3339(&row.try_get::<String, _>("updated_at")?)?,
    })
}

fn validate_occurrence(occurrence: &HabitOccurrence) -> CoreResult<()> {
    match (occurrence.scheduled_start_at, occurrence.scheduled_end_at) {
        (Some(start), Some(end)) if end <= start => Err(CoreError::Storage(
            "habit occurrence end must be after start".into(),
        )),
        (Some(_), None) | (None, Some(_)) => Err(CoreError::Storage(
            "habit occurrence schedule requires both start and end".into(),
        )),
        _ => Ok(()),
    }
}

#[async_trait::async_trait]
impl HabitOccurrenceRepository for PostgresHabitOccurrenceRepository {
    async fn upsert(&self, occurrence: HabitOccurrence) -> CoreResult<()> {
        validate_occurrence(&occurrence)?;
        sqlx::query(
            r#"
            INSERT INTO habit_occurrences (
                id, habit_id, occurrence_date, state, scheduled_start_at,
                scheduled_end_at, snoozed_until, skip_reason, created_at, updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            ON CONFLICT(habit_id, occurrence_date) DO UPDATE SET
                state = CASE
                    WHEN excluded.state = 'PENDING' THEN habit_occurrences.state
                    ELSE excluded.state
                END,
                scheduled_start_at = CASE
                    WHEN excluded.state = 'PENDING' THEN habit_occurrences.scheduled_start_at
                    ELSE excluded.scheduled_start_at
                END,
                scheduled_end_at = CASE
                    WHEN excluded.state = 'PENDING' THEN habit_occurrences.scheduled_end_at
                    ELSE excluded.scheduled_end_at
                END,
                snoozed_until = CASE
                    WHEN excluded.state = 'PENDING' THEN habit_occurrences.snoozed_until
                    ELSE excluded.snoozed_until
                END,
                skip_reason = CASE
                    WHEN excluded.state = 'PENDING' THEN habit_occurrences.skip_reason
                    ELSE excluded.skip_reason
                END,
                updated_at = CASE
                    WHEN excluded.state = 'PENDING' THEN habit_occurrences.updated_at
                    ELSE excluded.updated_at
                END
            "#,
        )
        .bind(occurrence.id.0)
        .bind(occurrence.habit_id.0)
        .bind(occurrence.occurrence_date)
        .bind(occurrence_state_to_str(occurrence.state))
        .bind(occurrence.scheduled_start_at)
        .bind(occurrence.scheduled_end_at)
        .bind(occurrence.snoozed_until)
        .bind(occurrence.skip_reason)
        .bind(occurrence.created_at)
        .bind(occurrence.updated_at)
        .execute(&self.pool)
        .await
        .map_err(map_storage_err)?;
        Ok(())
    }

    async fn find(&self, id: HabitOccurrenceId) -> CoreResult<Option<HabitOccurrence>> {
        sqlx::query("SELECT * FROM habit_occurrences WHERE id = $1")
            .bind(id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(map_storage_err)?
            .map(row_to_habit_occurrence_postgres)
            .transpose()
            .map_err(map_storage_err)
    }

    async fn find_for_date(
        &self,
        habit_id: HabitId,
        date: Date,
    ) -> CoreResult<Option<HabitOccurrence>> {
        sqlx::query("SELECT * FROM habit_occurrences WHERE habit_id = $1 AND occurrence_date = $2")
            .bind(habit_id.0)
            .bind(date)
            .fetch_optional(&self.pool)
            .await
            .map_err(map_storage_err)?
            .map(row_to_habit_occurrence_postgres)
            .transpose()
            .map_err(map_storage_err)
    }

    async fn list_for_habit_range(
        &self,
        habit_id: HabitId,
        start: Date,
        end_exclusive: Date,
    ) -> CoreResult<Vec<HabitOccurrence>> {
        if start >= end_exclusive {
            return Err(CoreError::Storage("invalid occurrence date range".into()));
        }
        sqlx::query(
            r#"
            SELECT * FROM habit_occurrences
            WHERE habit_id = $1 AND occurrence_date >= $2 AND occurrence_date < $3
            ORDER BY occurrence_date, id
            "#,
        )
        .bind(habit_id.0)
        .bind(start)
        .bind(end_exclusive)
        .fetch_all(&self.pool)
        .await
        .map_err(map_storage_err)?
        .into_iter()
        .map(row_to_habit_occurrence_postgres)
        .map(|result| result.map_err(map_storage_err))
        .collect()
    }
}

fn row_to_habit_occurrence_postgres(row: sqlx::postgres::PgRow) -> Result<HabitOccurrence> {
    Ok(HabitOccurrence {
        id: HabitOccurrenceId::from(row.try_get::<uuid::Uuid, _>("id")?),
        habit_id: HabitId::from(row.try_get::<uuid::Uuid, _>("habit_id")?),
        occurrence_date: row.try_get("occurrence_date")?,
        state: occurrence_state_from_str(&row.try_get::<String, _>("state")?),
        scheduled_start_at: row.try_get("scheduled_start_at")?,
        scheduled_end_at: row.try_get("scheduled_end_at")?,
        snoozed_until: row.try_get("snoozed_until")?,
        skip_reason: row.try_get("skip_reason")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

#[async_trait::async_trait]
impl HabitOccurrenceRepository for SqliteHabitOccurrenceRepository {
    async fn upsert(&self, occurrence: HabitOccurrence) -> CoreResult<()> {
        validate_occurrence(&occurrence)?;
        sqlx::query(
            r#"
            INSERT INTO habit_occurrences (
                id, habit_id, occurrence_date, state, scheduled_start_at,
                scheduled_end_at, snoozed_until, skip_reason, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(habit_id, occurrence_date) DO UPDATE SET
                state = CASE
                    WHEN excluded.state = 'PENDING' THEN habit_occurrences.state
                    ELSE excluded.state
                END,
                scheduled_start_at = CASE
                    WHEN excluded.state = 'PENDING' THEN habit_occurrences.scheduled_start_at
                    ELSE excluded.scheduled_start_at
                END,
                scheduled_end_at = CASE
                    WHEN excluded.state = 'PENDING' THEN habit_occurrences.scheduled_end_at
                    ELSE excluded.scheduled_end_at
                END,
                snoozed_until = CASE
                    WHEN excluded.state = 'PENDING' THEN habit_occurrences.snoozed_until
                    ELSE excluded.snoozed_until
                END,
                skip_reason = CASE
                    WHEN excluded.state = 'PENDING' THEN habit_occurrences.skip_reason
                    ELSE excluded.skip_reason
                END,
                updated_at = CASE
                    WHEN excluded.state = 'PENDING' THEN habit_occurrences.updated_at
                    ELSE excluded.updated_at
                END
            "#,
        )
        .bind(occurrence.id.0.to_string())
        .bind(occurrence.habit_id.0.to_string())
        .bind(date_to_string(occurrence.occurrence_date))
        .bind(occurrence_state_to_str(occurrence.state))
        .bind(optional_to_rfc3339(occurrence.scheduled_start_at).map_err(map_storage_err)?)
        .bind(optional_to_rfc3339(occurrence.scheduled_end_at).map_err(map_storage_err)?)
        .bind(optional_to_rfc3339(occurrence.snoozed_until).map_err(map_storage_err)?)
        .bind(occurrence.skip_reason)
        .bind(to_rfc3339(occurrence.created_at).map_err(map_storage_err)?)
        .bind(to_rfc3339(occurrence.updated_at).map_err(map_storage_err)?)
        .execute(&self.pool)
        .await
        .map_err(map_storage_err)?;
        Ok(())
    }

    async fn find(&self, id: HabitOccurrenceId) -> CoreResult<Option<HabitOccurrence>> {
        sqlx::query("SELECT * FROM habit_occurrences WHERE id = ?")
            .bind(id.0.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(map_storage_err)?
            .map(row_to_habit_occurrence_sqlite)
            .transpose()
            .map_err(map_storage_err)
    }

    async fn find_for_date(
        &self,
        habit_id: HabitId,
        date: Date,
    ) -> CoreResult<Option<HabitOccurrence>> {
        sqlx::query("SELECT * FROM habit_occurrences WHERE habit_id = ? AND occurrence_date = ?")
            .bind(habit_id.0.to_string())
            .bind(date_to_string(date))
            .fetch_optional(&self.pool)
            .await
            .map_err(map_storage_err)?
            .map(row_to_habit_occurrence_sqlite)
            .transpose()
            .map_err(map_storage_err)
    }

    async fn list_for_habit_range(
        &self,
        habit_id: HabitId,
        start: Date,
        end_exclusive: Date,
    ) -> CoreResult<Vec<HabitOccurrence>> {
        if start >= end_exclusive {
            return Err(CoreError::Storage("invalid occurrence date range".into()));
        }
        sqlx::query(
            r#"
            SELECT * FROM habit_occurrences
            WHERE habit_id = ? AND occurrence_date >= ? AND occurrence_date < ?
            ORDER BY occurrence_date, id
            "#,
        )
        .bind(habit_id.0.to_string())
        .bind(date_to_string(start))
        .bind(date_to_string(end_exclusive))
        .fetch_all(&self.pool)
        .await
        .map_err(map_storage_err)?
        .into_iter()
        .map(row_to_habit_occurrence_sqlite)
        .map(|result| result.map_err(map_storage_err))
        .collect()
    }
}

fn row_to_habit_occurrence_sqlite(row: sqlx::sqlite::SqliteRow) -> Result<HabitOccurrence> {
    Ok(HabitOccurrence {
        id: HabitOccurrenceId::from(uuid::Uuid::parse_str(&row.try_get::<String, _>("id")?)?),
        habit_id: HabitId::from(uuid::Uuid::parse_str(
            &row.try_get::<String, _>("habit_id")?,
        )?),
        occurrence_date: date_from_string(&row.try_get::<String, _>("occurrence_date")?)?,
        state: occurrence_state_from_str(&row.try_get::<String, _>("state")?),
        scheduled_start_at: optional_from_rfc3339(row.try_get("scheduled_start_at")?)?,
        scheduled_end_at: optional_from_rfc3339(row.try_get("scheduled_end_at")?)?,
        snoozed_until: optional_from_rfc3339(row.try_get("snoozed_until")?)?,
        skip_reason: row.try_get("skip_reason")?,
        created_at: from_rfc3339(&row.try_get::<String, _>("created_at")?)?,
        updated_at: from_rfc3339(&row.try_get::<String, _>("updated_at")?)?,
    })
}

#[async_trait::async_trait]
impl SchedulingPreferencesRepository for PostgresSchedulingPreferencesRepository {
    async fn upsert(&self, preferences: SchedulingPreferences) -> CoreResult<()> {
        preferences.validate().map_err(map_storage_err)?;
        let named_hours =
            serde_json::to_value(&preferences.named_hours).map_err(map_storage_err)?;
        let sleep = preferences
            .sleep
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(map_storage_err)?;
        sqlx::query(
            r#"
            INSERT INTO scheduling_preferences (
                id, user_id, timezone, named_hours, sleep,
                default_travel_buffer_minutes, created_at, updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT(user_id) DO UPDATE SET
                timezone = excluded.timezone,
                named_hours = excluded.named_hours,
                sleep = excluded.sleep,
                default_travel_buffer_minutes = excluded.default_travel_buffer_minutes,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(preferences.id.0)
        .bind(preferences.user_id.0)
        .bind(preferences.timezone)
        .bind(named_hours)
        .bind(sleep)
        .bind(u32_to_i32(
            preferences.default_travel_buffer_minutes,
            "default_travel_buffer_minutes",
        )?)
        .bind(preferences.created_at)
        .bind(preferences.updated_at)
        .execute(&self.pool)
        .await
        .map_err(map_storage_err)?;
        Ok(())
    }

    async fn get_for_user(&self, user_id: UserId) -> CoreResult<Option<SchedulingPreferences>> {
        sqlx::query("SELECT * FROM scheduling_preferences WHERE user_id = $1")
            .bind(user_id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(map_storage_err)?
            .map(row_to_scheduling_preferences_postgres)
            .transpose()
            .map_err(map_storage_err)
    }
}

fn row_to_scheduling_preferences_postgres(
    row: sqlx::postgres::PgRow,
) -> Result<SchedulingPreferences> {
    let named_hours: serde_json::Value = row.try_get("named_hours")?;
    let sleep: Option<serde_json::Value> = row.try_get("sleep")?;
    Ok(SchedulingPreferences {
        id: SchedulingPolicyId::from(row.try_get::<uuid::Uuid, _>("id")?),
        user_id: UserId::from(row.try_get::<uuid::Uuid, _>("user_id")?),
        timezone: row.try_get("timezone")?,
        named_hours: serde_json::from_value(named_hours)?,
        sleep: sleep.map(serde_json::from_value).transpose()?,
        default_travel_buffer_minutes: u32::try_from(
            row.try_get::<i32, _>("default_travel_buffer_minutes")?,
        )?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

#[async_trait::async_trait]
impl SchedulingPreferencesRepository for SqliteSchedulingPreferencesRepository {
    async fn upsert(&self, preferences: SchedulingPreferences) -> CoreResult<()> {
        preferences.validate().map_err(map_storage_err)?;
        let named_hours =
            serde_json::to_string(&preferences.named_hours).map_err(map_storage_err)?;
        let sleep = preferences
            .sleep
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(map_storage_err)?;
        sqlx::query(
            r#"
            INSERT INTO scheduling_preferences (
                id, user_id, timezone, named_hours, sleep,
                default_travel_buffer_minutes, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(user_id) DO UPDATE SET
                timezone = excluded.timezone,
                named_hours = excluded.named_hours,
                sleep = excluded.sleep,
                default_travel_buffer_minutes = excluded.default_travel_buffer_minutes,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(preferences.id.0.to_string())
        .bind(preferences.user_id.0.to_string())
        .bind(preferences.timezone)
        .bind(named_hours)
        .bind(sleep)
        .bind(i64::from(preferences.default_travel_buffer_minutes))
        .bind(to_rfc3339(preferences.created_at).map_err(map_storage_err)?)
        .bind(to_rfc3339(preferences.updated_at).map_err(map_storage_err)?)
        .execute(&self.pool)
        .await
        .map_err(map_storage_err)?;
        Ok(())
    }

    async fn get_for_user(&self, user_id: UserId) -> CoreResult<Option<SchedulingPreferences>> {
        sqlx::query("SELECT * FROM scheduling_preferences WHERE user_id = ?")
            .bind(user_id.0.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(map_storage_err)?
            .map(row_to_scheduling_preferences_sqlite)
            .transpose()
            .map_err(map_storage_err)
    }
}

fn row_to_scheduling_preferences_sqlite(
    row: sqlx::sqlite::SqliteRow,
) -> Result<SchedulingPreferences> {
    let sleep: Option<String> = row.try_get("sleep")?;
    Ok(SchedulingPreferences {
        id: SchedulingPolicyId::from(uuid::Uuid::parse_str(&row.try_get::<String, _>("id")?)?),
        user_id: UserId::from(uuid::Uuid::parse_str(
            &row.try_get::<String, _>("user_id")?,
        )?),
        timezone: row.try_get("timezone")?,
        named_hours: serde_json::from_str(&row.try_get::<String, _>("named_hours")?)?,
        sleep: sleep.as_deref().map(serde_json::from_str).transpose()?,
        default_travel_buffer_minutes: u32::try_from(
            row.try_get::<i64, _>("default_travel_buffer_minutes")?,
        )?,
        created_at: from_rfc3339(&row.try_get::<String, _>("created_at")?)?,
        updated_at: from_rfc3339(&row.try_get::<String, _>("updated_at")?)?,
    })
}
