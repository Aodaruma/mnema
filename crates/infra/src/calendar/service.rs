//! Calendar orchestration shared by native desktop and REST server.
use super::{CalendarError, CalendarWriteApi, GoogleCalendarAdapter};
use crate::db::Vault;
use anyhow::{Result, anyhow};
use mnema_core::prelude::*;
use serde::Serialize;
use time::{Duration, OffsetDateTime};

#[derive(Debug, Serialize)]
pub struct CalendarSyncResponse {
    pub account_id: String,
    pub calendars_synced: usize,
    pub full_syncs: usize,
    pub incremental_syncs: usize,
    pub events_upserted: usize,
    pub events_cancelled: usize,
}

#[derive(Debug, Serialize)]
pub struct CalendarWritebackResponse {
    pub account_id: String,
    pub calendar_id: String,
    pub created: usize,
    pub updated: usize,
    pub deleted: usize,
}

pub fn selected_sync_calendar_ids(account: &CalendarAccount) -> Vec<String> {
    account
        .selected_calendar_ids
        .iter()
        .filter(|calendar_id| {
            !calendar_id.trim().is_empty()
                && account.managed_calendar_id.as_deref() != Some(calendar_id.as_str())
        })
        .cloned()
        .collect()
}

pub async fn sync_selected_calendars(
    vault: &Vault,
    adapter: &GoogleCalendarAdapter,
    account: &CalendarAccount,
    start: OffsetDateTime,
    end: OffsetDateTime,
    force_full: bool,
) -> Result<CalendarSyncResponse> {
    let selected_calendar_ids = selected_sync_calendar_ids(account);
    if selected_calendar_ids.is_empty() {
        return Err(anyhow!(
            "Select at least one provider calendar before sync.",
        ));
    }
    let events = vault.external_event_repo();
    let cursors = vault.calendar_sync_cursor_repo();
    let now = OffsetDateTime::now_utc();
    let mut response = CalendarSyncResponse {
        account_id: account.id.0.to_string(),
        calendars_synced: 0,
        full_syncs: 0,
        incremental_syncs: 0,
        events_upserted: 0,
        events_cancelled: 0,
    };
    for calendar_id in &selected_calendar_ids {
        let cursor = cursors.get(account.id.clone(), calendar_id.clone()).await?;
        let (report, was_full) = if should_full_sync(force_full, cursor.as_ref(), now) {
            (
                adapter
                    .full_sync(
                        account,
                        calendar_id,
                        Some(start),
                        Some(end),
                        events.as_ref(),
                        cursors.as_ref(),
                    )
                    .await?,
                true,
            )
        } else {
            match adapter
                .incremental_sync(account, calendar_id, events.as_ref(), cursors.as_ref())
                .await
            {
                Ok(report) => (report, false),
                Err(CalendarError::FullSyncRequired | CalendarError::SyncTokenExpired) => (
                    adapter
                        .full_sync(
                            account,
                            calendar_id,
                            Some(start),
                            Some(end),
                            events.as_ref(),
                            cursors.as_ref(),
                        )
                        .await?,
                    true,
                ),
                Err(error) => return Err(error.into()),
            }
        };
        response.calendars_synced += 1;
        response.events_upserted += report.upserted;
        response.events_cancelled += report.cancelled;
        if was_full {
            response.full_syncs += 1;
        } else {
            response.incremental_syncs += 1;
        }
    }
    Ok(response)
}

pub fn should_full_sync(
    force_full: bool,
    cursor: Option<&CalendarSyncCursor>,
    now: OffsetDateTime,
) -> bool {
    force_full
        || !cursor.is_some_and(|cursor| {
            cursor.sync_token.is_some()
                && cursor
                    .last_full_sync_at
                    .is_some_and(|last| last >= now - Duration::hours(12))
        })
}

pub fn is_writeback_account(account: &CalendarAccount) -> bool {
    account.enabled
        && account.access_mode == CalendarAccessMode::ReadWrite
        && account
            .managed_calendar_id
            .as_deref()
            .is_some_and(|calendar_id| !calendar_id.trim().is_empty())
}

pub async fn writeback_schedule(
    vault: &Vault,
    adapter: &dyn CalendarWriteApi,
    account: &CalendarAccount,
    start: OffsetDateTime,
    end: OffsetDateTime,
    timezone: &str,
) -> Result<CalendarWritebackResponse> {
    if account.access_mode != CalendarAccessMode::ReadWrite {
        return Err(anyhow!("Calendar account does not allow write-back.",));
    }
    let calendar_id = account
        .managed_calendar_id
        .clone()
        .filter(|calendar_id| !calendar_id.trim().is_empty())
        .ok_or_else(|| anyhow!("Create a managed calendar before write-back."))?;
    let schedule_repo = vault.schedule_block_repo();
    let links_repo = vault.managed_calendar_event_link_repo();
    let blocks = schedule_repo
        .list_overlapping(start, end)
        .await?
        .into_iter()
        .filter(is_writeback_block)
        .collect::<Vec<_>>();
    let mut response = CalendarWritebackResponse {
        account_id: account.id.0.to_string(),
        calendar_id: calendar_id.clone(),
        created: 0,
        updated: 0,
        deleted: 0,
    };
    let now = OffsetDateTime::now_utc();
    for block in &blocks {
        let update_draft = super::CalendarEventDraft {
            provider_event_id: None,
            title: block
                .title_snapshot
                .clone()
                .unwrap_or_else(|| "Mnema focus block".to_string()),
            description: Some("Managed by Mnema".to_string()),
            location: None,
            start_at: block.start_at,
            end_at: block.end_at,
            all_day: false,
            timezone: Some(timezone.to_string()),
        };
        let create_draft = super::CalendarEventDraft {
            provider_event_id: Some(managed_provider_event_id(&block.id)),
            ..update_draft.clone()
        };
        let existing = links_repo
            .find_by_schedule_block(account.id.clone(), block.id.clone())
            .await?;
        let (event, mut link) = if let Some(existing) = existing {
            if existing.calendar_id == calendar_id {
                let event = match adapter
                    .update_event(
                        account,
                        &calendar_id,
                        &existing.provider_event_id,
                        existing.etag.as_deref(),
                        update_draft,
                    )
                    .await
                {
                    Ok(event) => {
                        response.updated += 1;
                        event
                    }
                    Err(CalendarError::Provider { status: 404, .. }) => {
                        response.created += 1;
                        adapter
                            .create_event(account, &calendar_id, create_draft)
                            .await?
                    }
                    Err(error) => return Err(error.into()),
                };
                (event, existing)
            } else {
                match adapter
                    .delete_event(
                        account,
                        &existing.calendar_id,
                        &existing.provider_event_id,
                        existing.etag.as_deref(),
                    )
                    .await
                {
                    Ok(()) | Err(CalendarError::Provider { status: 404, .. }) => {}
                    Err(error) => return Err(error.into()),
                }
                let event = adapter
                    .create_event(account, &calendar_id, create_draft)
                    .await?;
                response.created += 1;
                (event, existing)
            }
        } else {
            let event = adapter
                .create_event(account, &calendar_id, create_draft)
                .await?;
            response.created += 1;
            (
                event,
                ManagedCalendarEventLink {
                    id: ManagedCalendarEventId::new(),
                    account_id: account.id.clone(),
                    schedule_block_id: block.id.clone(),
                    calendar_id: calendar_id.clone(),
                    provider_event_id: String::new(),
                    etag: None,
                    created_at: now,
                    updated_at: now,
                },
            )
        };
        link.calendar_id = calendar_id.clone();
        link.provider_event_id = event.provider_event_id;
        link.etag = event.etag;
        link.updated_at = now;
        links_repo.upsert(link).await?;
    }

    let active_ids = blocks
        .iter()
        .map(|block| block.id.clone())
        .collect::<std::collections::HashSet<_>>();
    for link in links_repo.list_for_account(account.id.clone()).await? {
        if active_ids.contains(&link.schedule_block_id) {
            continue;
        }
        let block = schedule_repo.find(link.schedule_block_id.clone()).await?;
        let should_delete = block.as_ref().is_none_or(|block| {
            (!is_writeback_block(block) && block.start_at < end && block.end_at > start)
                || matches!(
                    block.state,
                    ScheduleBlockState::Cancelled | ScheduleBlockState::Missed
                )
        });
        if !should_delete {
            continue;
        }
        match adapter
            .delete_event(
                account,
                &link.calendar_id,
                &link.provider_event_id,
                link.etag.as_deref(),
            )
            .await
        {
            Ok(()) | Err(CalendarError::Provider { status: 404, .. }) => {}
            Err(error) => return Err(error.into()),
        }
        links_repo.delete(link.id).await?;
        response.deleted += 1;
    }
    Ok(response)
}

pub fn managed_provider_event_id(block_id: &ScheduleBlockId) -> String {
    format!("mnema{}", block_id.0.to_string().replace('-', "")).to_ascii_lowercase()
}

pub fn is_writeback_block(block: &ScheduleBlock) -> bool {
    matches!(
        block.source,
        ScheduleBlockSource::Scheduler | ScheduleBlockSource::Repair
    ) && !matches!(
        block.state,
        ScheduleBlockState::Cancelled | ScheduleBlockState::Missed
    ) && block.end_at > block.start_at
}
