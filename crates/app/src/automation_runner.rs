use std::sync::Arc;

use async_trait::async_trait;
use mnema_core::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    AppResult, AutoScheduleApplyResult, AutoSchedulePreview, AutoScheduleRequest,
    AutoScheduleService, TimeWindow, iana_date_range,
};

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CalendarSyncStepResult {
    pub accounts_synced: u32,
    pub events_upserted: u32,
    pub full_syncs: u32,
    pub incremental_syncs: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CalendarWritebackStepResult {
    pub created: u32,
    pub updated: u32,
    pub deleted: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationRunRequest {
    pub schedule: AutoScheduleRequest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationRunResult {
    pub sync: CalendarSyncStepResult,
    pub preview: AutoSchedulePreview,
    pub apply: AutoScheduleApplyResult,
    pub writeback: CalendarWritebackStepResult,
}

/// Provider-specific synchronization (Google, Outlook, CalDAV, etc.).
#[async_trait]
pub trait CalendarSyncStep: Send + Sync {
    async fn sync(&self, planning_window: TimeWindow) -> AppResult<CalendarSyncStepResult>;
}

/// Planning abstraction kept separate so the automation runner is easy to test
/// and can later run in a queue/worker without depending on a concrete adapter.
#[async_trait]
pub trait AutoScheduleStep: Send + Sync {
    async fn preview(&self, request: AutoScheduleRequest) -> AppResult<AutoSchedulePreview>;
    async fn apply(
        &self,
        preview: &AutoSchedulePreview,
        expected_fingerprint: &str,
    ) -> AppResult<AutoScheduleApplyResult>;
}

#[async_trait]
impl AutoScheduleStep for AutoScheduleService<'_> {
    async fn preview(&self, request: AutoScheduleRequest) -> AppResult<AutoSchedulePreview> {
        AutoScheduleService::preview(self, request).await
    }

    async fn apply(
        &self,
        preview: &AutoSchedulePreview,
        expected_fingerprint: &str,
    ) -> AppResult<AutoScheduleApplyResult> {
        AutoScheduleService::apply(self, preview, expected_fingerprint).await
    }
}

/// Provider-specific create/update/delete of Mnema-managed calendar events.
#[async_trait]
pub trait CalendarWritebackStep: Send + Sync {
    async fn writeback(
        &self,
        applied_blocks: &[ScheduleBlock],
    ) -> AppResult<CalendarWritebackStepResult>;
}

pub struct AutomationRunner<'a> {
    sync: &'a dyn CalendarSyncStep,
    schedule: &'a dyn AutoScheduleStep,
    writeback: &'a dyn CalendarWritebackStep,
}

impl<'a> AutomationRunner<'a> {
    #[must_use]
    pub fn new(
        sync: &'a dyn CalendarSyncStep,
        schedule: &'a dyn AutoScheduleStep,
        writeback: &'a dyn CalendarWritebackStep,
    ) -> Self {
        Self {
            sync,
            schedule,
            writeback,
        }
    }

    /// Executes the S3 pipeline in its observable order: sync, preview/apply,
    /// then write-back. Provider implementations own retry/idempotency details.
    pub async fn run(&self, request: AutomationRunRequest) -> AppResult<AutomationRunResult> {
        let timezone = request.schedule.timezone.clone();
        let planning_window = iana_date_range(
            request.schedule.start_date,
            request.schedule.end_date_exclusive,
            &timezone,
        )?;
        let sync = self.sync.sync(planning_window).await?;
        let preview = self.schedule.preview(request.schedule).await?;
        let fingerprint = preview.diff.fingerprint.clone();
        let apply = self.schedule.apply(&preview, &fingerprint).await?;
        let writeback = self.writeback.writeback(&apply.blocks).await?;
        Ok(AutomationRunResult {
            sync,
            preview,
            apply,
            writeback,
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct NoopCalendarSync;

#[async_trait]
impl CalendarSyncStep for NoopCalendarSync {
    async fn sync(&self, _planning_window: TimeWindow) -> AppResult<CalendarSyncStepResult> {
        Ok(CalendarSyncStepResult::default())
    }
}

#[derive(Debug, Clone, Default)]
pub struct NoopCalendarWriteback;

#[async_trait]
impl CalendarWritebackStep for NoopCalendarWriteback {
    async fn writeback(
        &self,
        _applied_blocks: &[ScheduleBlock],
    ) -> AppResult<CalendarWritebackStepResult> {
        Ok(CalendarWritebackStepResult::default())
    }
}

/// Shareable aliases useful for server background workers.
pub type SharedCalendarSyncStep = Arc<dyn CalendarSyncStep>;
pub type SharedAutoScheduleStep = Arc<dyn AutoScheduleStep>;
pub type SharedCalendarWritebackStep = Arc<dyn CalendarWritebackStep>;

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use time::macros::{date, datetime};

    use super::*;
    use crate::{AutoScheduleDiff, ItemSchedulingOutput};

    struct FakeSync<'a>(&'a Mutex<Vec<&'static str>>);

    #[async_trait]
    impl CalendarSyncStep for FakeSync<'_> {
        async fn sync(&self, _planning_window: TimeWindow) -> AppResult<CalendarSyncStepResult> {
            self.0.lock().unwrap().push("sync");
            Ok(CalendarSyncStepResult {
                accounts_synced: 1,
                ..Default::default()
            })
        }
    }

    struct FakeSchedule<'a>(&'a Mutex<Vec<&'static str>>);

    #[async_trait]
    impl AutoScheduleStep for FakeSchedule<'_> {
        async fn preview(&self, request: AutoScheduleRequest) -> AppResult<AutoSchedulePreview> {
            self.0.lock().unwrap().push("preview");
            Ok(AutoSchedulePreview {
                planning_window: iana_date_range(
                    request.start_date,
                    request.end_date_exclusive,
                    &request.timezone,
                )?,
                request,
                availability: vec![],
                hard_busy: vec![],
                new_habit_occurrences: vec![],
                output: ItemSchedulingOutput {
                    blocks: vec![],
                    unscheduled: vec![],
                    issues: vec![],
                },
                proposed_blocks: vec![],
                diff: AutoScheduleDiff {
                    fingerprint: "expected".into(),
                    changes: vec![],
                },
            })
        }

        async fn apply(
            &self,
            preview: &AutoSchedulePreview,
            expected_fingerprint: &str,
        ) -> AppResult<AutoScheduleApplyResult> {
            self.0.lock().unwrap().push("apply");
            assert_eq!(expected_fingerprint, "expected");
            Ok(AutoScheduleApplyResult {
                planning_window: preview.planning_window,
                fingerprint: expected_fingerprint.into(),
                blocks: vec![],
            })
        }
    }

    struct FakeWriteback<'a>(&'a Mutex<Vec<&'static str>>);

    #[async_trait]
    impl CalendarWritebackStep for FakeWriteback<'_> {
        async fn writeback(
            &self,
            _applied_blocks: &[ScheduleBlock],
        ) -> AppResult<CalendarWritebackStepResult> {
            self.0.lock().unwrap().push("writeback");
            Ok(CalendarWritebackStepResult {
                created: 1,
                ..Default::default()
            })
        }
    }

    #[tokio::test]
    async fn runs_sync_replan_and_writeback_in_order() {
        let calls = Mutex::new(Vec::new());
        let sync = FakeSync(&calls);
        let schedule = FakeSchedule(&calls);
        let writeback = FakeWriteback(&calls);
        let result = AutomationRunner::new(&sync, &schedule, &writeback)
            .run(AutomationRunRequest {
                schedule: AutoScheduleRequest {
                    user_id: UserId::new(),
                    start_date: date!(2026 - 08 - 15),
                    end_date_exclusive: date!(2026 - 08 - 16),
                    timezone: "Asia/Tokyo".into(),
                    named_hours: vec![],
                },
            })
            .await
            .unwrap();

        assert_eq!(
            *calls.lock().unwrap(),
            ["sync", "preview", "apply", "writeback"]
        );
        assert_eq!(result.sync.accounts_synced, 1);
        assert_eq!(result.writeback.created, 1);
        assert_eq!(
            result.apply.planning_window.start,
            datetime!(2026-08-15 00:00 +09:00)
        );
    }
}
