use mnema_app::AutoScheduleRequest;
use mnema_core::prelude::*;
use serde::Serialize;
use time::{Duration, OffsetDateTime};
use time_tz::{OffsetDateTimeExt, timezones};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    api::AppState,
    calendar_api::{
        enabled_calendar_accounts, is_writeback_account, selected_sync_calendar_ids,
        sync_selected_calendars, writeback_schedule,
    },
    config::AutomationMode,
    scheduling_api::{apply_auto_schedule, preview_auto_schedule},
};

pub(crate) fn spawn(state: AppState) {
    let interval_seconds = state.config.automation_interval_seconds;
    let mode = state.config.automation_mode;
    if interval_seconds == 0 || mode == AutomationMode::Off {
        info!(mode = mode.as_str(), "automation worker disabled");
        return;
    }

    tokio::spawn(async move {
        run_once(&state).await;
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(interval_seconds));
        ticker.tick().await;
        loop {
            ticker.tick().await;
            run_once(&state).await;
        }
    });
}

async fn run_once(state: &AppState) {
    let summary = run_pipeline(state).await;
    record_automation_log(state, &summary).await;
}

async fn run_pipeline(state: &AppState) -> WorkerRunSummary {
    let gate = automation_gate(state.config.automation_mode);
    let mut summary = WorkerRunSummary::new(state.config.automation_mode);
    let preferences = match state
        .vault
        .scheduling_preferences_repo()
        .get_for_user(local_user_id())
        .await
    {
        Ok(Some(preferences)) => preferences,
        Ok(None) => {
            warn!("automation skipped because scheduling preferences are missing");
            return summary.failed("scheduling preferences are missing");
        }
        Err(error) => {
            warn!(%error, "automation preferences lookup failed");
            return summary.failed(format!("preferences lookup failed: {error}"));
        }
    };
    let Some(timezone) = timezones::get_by_name(&preferences.timezone) else {
        warn!(timezone = %preferences.timezone, "automation skipped because timezone is invalid");
        return summary.failed(format!("timezone is invalid: {}", preferences.timezone));
    };
    let timezone_name = preferences.timezone.clone();
    let start_date = OffsetDateTime::now_utc().to_timezone(timezone).date();
    let Some(end_date_exclusive) = start_date.checked_add(Duration::days(7)) else {
        warn!("automation skipped because date range overflowed");
        return summary.failed("date range overflowed");
    };

    // Preserve cached data for display, but do not plan against a failed sync.
    let accounts = match enabled_calendar_accounts(state).await {
        Ok(accounts) => accounts,
        Err(error) => {
            warn!(status = %error.status, error = %error.message, "calendar account lookup failed; continuing with last snapshot");
            summary.mark_warning("calendar account lookup failed");
            Vec::new()
        }
    };
    summary.accounts_discovered = accounts.len();
    for account in &accounts {
        if selected_sync_calendar_ids(account).is_empty() {
            continue;
        }
        match sync_selected_calendars(state, account, start_date, end_date_exclusive, false).await {
            Ok(result) => {
                summary.accounts_synced += 1;
                summary.events_upserted += result.events_upserted;
                summary.events_cancelled += result.events_cancelled;
                info!(
                    account_id = %result.account_id,
                    calendars = result.calendars_synced,
                    full = result.full_syncs,
                    incremental = result.incremental_syncs,
                    "automation calendar sync completed"
                );
            }
            Err(error) => {
                summary.sync_failures += 1;
                summary.mark_warning("one or more calendar syncs failed");
                warn!(
                    account_id = %account.id.0,
                    status = %error.status,
                    error = %error.message,
                    "calendar sync failed; automatic planning will be skipped"
                );
            }
        }
    }

    if summary.sync_failures > 0 {
        return summary.failed("calendar sync failed; planning and write-back skipped");
    }

    let request = AutoScheduleRequest {
        user_id: local_user_id(),
        start_date,
        end_date_exclusive,
        timezone: timezone_name.clone(),
        named_hours: Vec::new(),
        not_before: Some(OffsetDateTime::now_utc()),
    };
    let preview = match preview_auto_schedule(state, request).await {
        Ok(preview) => preview,
        Err(error) => {
            warn!(%error, "automation preview failed");
            return summary.failed(format!("preview failed: {error}"));
        }
    };
    summary.preview_changes = Some(preview.diff.changed_count());
    info!(
        mode = state.config.automation_mode.as_str(),
        changed = preview.diff.changed_count(),
        "automation preview completed"
    );

    // Suggest mode intentionally ends after sync + preview.
    if !gate.apply_schedule {
        if summary.explanation.is_empty() {
            summary.explanation =
                "calendar sync and preview completed; mutation gate is closed".into();
        }
        return summary;
    }

    if preview.diff.has_changes() {
        let fingerprint = preview.diff.fingerprint.clone();
        match apply_auto_schedule(state, &preview, &fingerprint).await {
            Ok(result) => {
                summary.applied_blocks = result.blocks.len();
                info!(blocks = result.blocks.len(), "automation plan applied");
            }
            Err(error) => {
                warn!(%error, "automation apply failed");
                return summary.failed(format!("apply failed: {error}"));
            }
        }
    }

    if gate.writeback_calendar {
        for account in accounts
            .iter()
            .filter(|account| is_writeback_account(account))
        {
            match writeback_schedule(
                state,
                account,
                start_date,
                end_date_exclusive,
                &timezone_name,
            )
            .await
            {
                Ok(result) => {
                    summary.writeback_accounts += 1;
                    summary.writeback_created += result.created;
                    summary.writeback_updated += result.updated;
                    summary.writeback_deleted += result.deleted;
                    info!(
                        account_id = %result.account_id,
                        calendar_id = %result.calendar_id,
                        created = result.created,
                        updated = result.updated,
                        deleted = result.deleted,
                        "automation calendar write-back completed"
                    );
                }
                Err(error) => {
                    summary.writeback_failures += 1;
                    summary.mark_warning("one or more calendar write-backs failed");
                    warn!(
                        account_id = %account.id.0,
                        status = %error.status,
                        error = %error.message,
                        "automation calendar write-back failed"
                    );
                }
            }
        }
    }
    if summary.explanation.is_empty() {
        summary.explanation = "sync, preview, apply gate, and write-back completed".into();
    }
    summary
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AutomationGate {
    apply_schedule: bool,
    writeback_calendar: bool,
}

const fn automation_gate(mode: AutomationMode) -> AutomationGate {
    match mode {
        AutomationMode::Off | AutomationMode::Suggest => AutomationGate {
            apply_schedule: false,
            writeback_calendar: false,
        },
        AutomationMode::AutoSilent => AutomationGate {
            apply_schedule: true,
            writeback_calendar: true,
        },
    }
}

#[derive(Debug, Serialize)]
struct WorkerRunSummary {
    status: String,
    mode: String,
    accounts_discovered: usize,
    accounts_synced: usize,
    sync_failures: usize,
    events_upserted: usize,
    events_cancelled: usize,
    preview_changes: Option<usize>,
    applied_blocks: usize,
    writeback_accounts: usize,
    writeback_created: usize,
    writeback_updated: usize,
    writeback_deleted: usize,
    writeback_failures: usize,
    explanation: String,
}

impl WorkerRunSummary {
    fn new(mode: AutomationMode) -> Self {
        Self {
            status: "SUCCESS".into(),
            mode: mode.as_str().into(),
            accounts_discovered: 0,
            accounts_synced: 0,
            sync_failures: 0,
            events_upserted: 0,
            events_cancelled: 0,
            preview_changes: None,
            applied_blocks: 0,
            writeback_accounts: 0,
            writeback_created: 0,
            writeback_updated: 0,
            writeback_deleted: 0,
            writeback_failures: 0,
            explanation: String::new(),
        }
    }

    fn mark_warning(&mut self, explanation: &str) {
        if self.status != "FAILED" {
            self.status = "SUCCESS_WITH_WARNINGS".into();
        }
        if self.explanation.is_empty() {
            self.explanation = explanation.into();
        }
    }

    fn failed(mut self, explanation: impl Into<String>) -> Self {
        self.status = "FAILED".into();
        self.explanation = explanation.into();
        self
    }
}

async fn record_automation_log(state: &AppState, summary: &WorkerRunSummary) {
    let Some(assistant_id) = configured_automation_assistant_id() else {
        return;
    };
    let after_state = match serde_json::to_value(summary) {
        Ok(value) => Some(value),
        Err(error) => {
            warn!(%error, "automation summary serialization failed");
            None
        }
    };
    let log = AutomationLog {
        id: AutomationLogId::new(),
        task_id: None,
        project_id: None,
        list_id: None,
        assistant_id,
        action_type: AutomationActionType::Other("AUTO_SCHEDULE_PIPELINE".into()),
        before_state: None,
        after_state,
        created_at: OffsetDateTime::now_utc(),
        explanation: Some(summary.explanation.clone()),
    };
    if let Err(error) = state.vault.automation_log_repo().insert(log).await {
        warn!(%error, "automation log persistence failed");
    }
}

fn configured_automation_assistant_id() -> Option<AssistantId> {
    let value = std::env::var("MNEMA_AUTOMATION_ASSISTANT_ID").ok()?;
    match Uuid::parse_str(value.trim()) {
        Ok(id) => Some(AssistantId::from(id)),
        Err(error) => {
            warn!(%error, "MNEMA_AUTOMATION_ASSISTANT_ID is invalid; automation log disabled");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggest_can_sync_and_preview_but_never_mutates() {
        assert_eq!(
            automation_gate(AutomationMode::Suggest),
            AutomationGate {
                apply_schedule: false,
                writeback_calendar: false,
            }
        );
    }

    #[test]
    fn only_explicit_auto_silent_mode_opens_both_mutation_gates() {
        assert_eq!(
            automation_gate(AutomationMode::Off),
            AutomationGate {
                apply_schedule: false,
                writeback_calendar: false,
            }
        );
        assert_eq!(
            automation_gate(AutomationMode::AutoSilent),
            AutomationGate {
                apply_schedule: true,
                writeback_calendar: true,
            }
        );
    }
}
