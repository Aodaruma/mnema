use crate::automation_log::AutomationLog;
use crate::calendar::{
    CalendarAccount, CalendarProvider, CalendarSyncCursor, ExternalEvent, ManagedCalendarEventLink,
};
use crate::habit::{Habit, HabitOccurrence};
use crate::list::List;
use crate::milestone::Milestone;
use crate::project::Project;
use crate::schedule_block::ScheduleBlock;
use crate::scheduling::SchedulingPreferences;
use crate::status::{Status, StatusGroup};
use crate::task::Task;
use crate::{
    ids::{
        AutomationLogId, CalendarAccountId, ExternalEventId, HabitId, HabitOccurrenceId, ListId,
        ManagedCalendarEventId, MilestoneId, ProjectId, ScheduleBlockId, TaskId, UserId,
    },
    user_settings::UserSettings,
};
use thiserror::Error;
use time::{Date, OffsetDateTime};

pub type CoreResult<T> = Result<T, CoreError>;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("not found")]
    NotFound,
    #[error("storage error: {0}")]
    Storage(String),
}

#[async_trait::async_trait]
pub trait TaskRepository: Send + Sync {
    async fn insert(&self, task: Task) -> CoreResult<()>;
    async fn find(&self, id: TaskId) -> CoreResult<Option<Task>>;
    async fn update(&self, task: Task) -> CoreResult<()>;
    async fn list_all(&self) -> CoreResult<Vec<Task>>;
    async fn list_by_project(&self, project_id: ProjectId) -> CoreResult<Vec<Task>>;
    async fn list_by_list(&self, list_id: ListId) -> CoreResult<Vec<Task>>;
    async fn soft_delete(&self, id: TaskId, deleted_at: OffsetDateTime) -> CoreResult<()>;
}

#[async_trait::async_trait]
pub trait ProjectRepository: Send + Sync {
    async fn insert(&self, project: Project) -> CoreResult<()>;
    async fn find(&self, id: ProjectId) -> CoreResult<Option<Project>>;
    async fn list_all(&self) -> CoreResult<Vec<Project>>;
    async fn update(&self, project: Project) -> CoreResult<()>;
}

#[async_trait::async_trait]
pub trait ListRepository: Send + Sync {
    async fn insert(&self, list: List) -> CoreResult<()>;
    async fn find(&self, id: ListId) -> CoreResult<Option<List>>;
    async fn list_by_project(&self, project_id: ProjectId) -> CoreResult<Vec<List>>;
    async fn list_system(&self) -> CoreResult<Vec<List>>;
    async fn update(&self, list: List) -> CoreResult<()>;
}

#[async_trait::async_trait]
pub trait MilestoneRepository: Send + Sync {
    async fn insert(&self, milestone: Milestone) -> CoreResult<()>;
    async fn find(&self, id: MilestoneId) -> CoreResult<Option<Milestone>>;
    async fn list_by_project(&self, project_id: ProjectId) -> CoreResult<Vec<Milestone>>;
    async fn update(&self, milestone: Milestone) -> CoreResult<()>;
}

#[async_trait::async_trait]
pub trait StatusRepository: Send + Sync {
    async fn insert_group(&self, group: StatusGroup) -> CoreResult<()>;
    async fn insert_status(&self, status: Status) -> CoreResult<()>;
    async fn list_groups(&self) -> CoreResult<Vec<StatusGroup>>;
    async fn list_statuses_for_project(
        &self,
        project_id: Option<ProjectId>,
    ) -> CoreResult<Vec<Status>>;
}

#[async_trait::async_trait]
pub trait ScheduleBlockRepository: Send + Sync {
    async fn insert(&self, block: ScheduleBlock) -> CoreResult<()>;
    async fn find(&self, id: ScheduleBlockId) -> CoreResult<Option<ScheduleBlock>>;
    async fn update(&self, block: ScheduleBlock) -> CoreResult<()>;
    async fn list_for_day(&self, day: Date) -> CoreResult<Vec<ScheduleBlock>>;
    async fn list_overlapping(
        &self,
        _start: OffsetDateTime,
        _end: OffsetDateTime,
    ) -> CoreResult<Vec<ScheduleBlock>> {
        Err(CoreError::Storage(
            "list_overlapping is not implemented by this repository".into(),
        ))
    }
    async fn replace_proposed_for_day(
        &self,
        day: Date,
        blocks: Vec<ScheduleBlock>,
    ) -> CoreResult<()>;
    async fn replace_proposed_in_range(
        &self,
        _start: OffsetDateTime,
        _end: OffsetDateTime,
        _blocks: Vec<ScheduleBlock>,
    ) -> CoreResult<()> {
        Err(CoreError::Storage(
            "replace_proposed_in_range is not implemented by this repository".into(),
        ))
    }
}

#[async_trait::async_trait]
pub trait CalendarAccountRepository: Send + Sync {
    async fn upsert(&self, account: CalendarAccount) -> CoreResult<()>;
    async fn find(&self, id: CalendarAccountId) -> CoreResult<Option<CalendarAccount>>;
    async fn find_by_provider_identity(
        &self,
        provider: CalendarProvider,
        provider_account_id: String,
    ) -> CoreResult<Option<CalendarAccount>>;
    async fn list_enabled(&self, user_id: UserId) -> CoreResult<Vec<CalendarAccount>>;
}

#[async_trait::async_trait]
pub trait ExternalEventRepository: Send + Sync {
    /// Idempotent on `(account_id, calendar_id, provider_event_id)`.
    async fn upsert(&self, event: ExternalEvent) -> CoreResult<()>;
    async fn find(&self, id: ExternalEventId) -> CoreResult<Option<ExternalEvent>>;
    async fn find_by_provider_event(
        &self,
        account_id: CalendarAccountId,
        calendar_id: String,
        provider_event_id: String,
    ) -> CoreResult<Option<ExternalEvent>>;
    async fn list_overlapping(
        &self,
        start: OffsetDateTime,
        end: OffsetDateTime,
    ) -> CoreResult<Vec<ExternalEvent>>;
    async fn list_overlapping_for_account(
        &self,
        account_id: CalendarAccountId,
        start: OffsetDateTime,
        end: OffsetDateTime,
    ) -> CoreResult<Vec<ExternalEvent>>;
    /// Completes a successful full-sync snapshot while preserving rows that
    /// were upserted (and therefore their local travel overrides).
    async fn delete_unseen_for_calendar(
        &self,
        _account_id: CalendarAccountId,
        _calendar_id: String,
        _retained_provider_event_ids: Vec<String>,
    ) -> CoreResult<usize> {
        Err(CoreError::Storage(
            "full-sync snapshot reconciliation is not implemented by this repository".into(),
        ))
    }
}

#[async_trait::async_trait]
pub trait CalendarSyncCursorRepository: Send + Sync {
    async fn upsert(&self, cursor: CalendarSyncCursor) -> CoreResult<()>;
    async fn get(
        &self,
        account_id: CalendarAccountId,
        calendar_id: String,
    ) -> CoreResult<Option<CalendarSyncCursor>>;
    async fn clear(&self, account_id: CalendarAccountId, calendar_id: String) -> CoreResult<()>;
}

#[async_trait::async_trait]
pub trait ManagedCalendarEventLinkRepository: Send + Sync {
    /// Idempotent on both the local schedule block and remote event identity.
    async fn upsert(&self, link: ManagedCalendarEventLink) -> CoreResult<()>;
    async fn find(
        &self,
        id: ManagedCalendarEventId,
    ) -> CoreResult<Option<ManagedCalendarEventLink>>;
    async fn find_by_schedule_block(
        &self,
        account_id: CalendarAccountId,
        schedule_block_id: ScheduleBlockId,
    ) -> CoreResult<Option<ManagedCalendarEventLink>>;
    async fn find_by_provider_event(
        &self,
        account_id: CalendarAccountId,
        calendar_id: String,
        provider_event_id: String,
    ) -> CoreResult<Option<ManagedCalendarEventLink>>;
    async fn list_for_account(
        &self,
        account_id: CalendarAccountId,
    ) -> CoreResult<Vec<ManagedCalendarEventLink>>;
    async fn delete(&self, id: ManagedCalendarEventId) -> CoreResult<()>;
}

#[async_trait::async_trait]
pub trait HabitRepository: Send + Sync {
    async fn upsert(&self, habit: Habit) -> CoreResult<()>;
    async fn find(&self, id: HabitId) -> CoreResult<Option<Habit>>;
    async fn list_enabled(&self, user_id: UserId) -> CoreResult<Vec<Habit>>;
}

#[async_trait::async_trait]
pub trait HabitOccurrenceRepository: Send + Sync {
    /// Idempotent on `(habit_id, occurrence_date)`.
    async fn upsert(&self, occurrence: HabitOccurrence) -> CoreResult<()>;
    async fn find(&self, id: HabitOccurrenceId) -> CoreResult<Option<HabitOccurrence>>;
    async fn find_for_date(
        &self,
        habit_id: HabitId,
        date: Date,
    ) -> CoreResult<Option<HabitOccurrence>>;
    async fn list_for_habit_range(
        &self,
        habit_id: HabitId,
        start: Date,
        end_exclusive: Date,
    ) -> CoreResult<Vec<HabitOccurrence>>;
}

#[async_trait::async_trait]
pub trait SchedulingPreferencesRepository: Send + Sync {
    async fn upsert(&self, preferences: SchedulingPreferences) -> CoreResult<()>;
    async fn get_for_user(&self, user_id: UserId) -> CoreResult<Option<SchedulingPreferences>>;
}

#[async_trait::async_trait]
pub trait UserSettingsRepository: Send + Sync {
    async fn upsert(&self, settings: UserSettings) -> CoreResult<()>;
    async fn get(&self, user_id: crate::ids::UserId) -> CoreResult<Option<UserSettings>>;
}

#[async_trait::async_trait]
pub trait AutomationLogRepository: Send + Sync {
    async fn insert(&self, log: AutomationLog) -> CoreResult<()>;
    async fn find(&self, id: AutomationLogId) -> CoreResult<Option<AutomationLog>>;
    async fn list_recent(&self, limit: u32) -> CoreResult<Vec<AutomationLog>>;
}
