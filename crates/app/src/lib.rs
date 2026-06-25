//! Application services that connect Mnema domain repositories to pure logic.

use std::collections::{HashMap, HashSet};

use mnema_core::prelude::*;
use mnema_scheduler::{ScheduleIssue, SchedulingOutput};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::{Date, OffsetDateTime};

pub use mnema_scheduler::{
    AvailabilityWindow, BusyBlock, BusyBlockSource, GreedyScheduler, ProposedScheduleBlock,
    SchedulerConfig, SchedulingInput, TimeWindow,
};

#[derive(Debug, Error)]
pub enum AppError {
    #[error("repository error: {0}")]
    Repository(String),
    #[error("missing default inbox list")]
    MissingDefaultInbox,
    #[error("missing default task status")]
    MissingDefaultStatus,
    #[error("task title is required")]
    EmptyTaskTitle,
}

impl From<CoreError> for AppError {
    fn from(value: CoreError) -> Self {
        Self::Repository(value.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureTaskRequest {
    pub title: String,
    pub description: Option<String>,
    pub due_date: Option<Date>,
    pub estimated_minutes: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureTaskResult {
    pub task: Task,
}

pub struct CaptureTaskService<'a> {
    tasks: &'a dyn TaskRepository,
    lists: &'a dyn ListRepository,
    statuses: &'a dyn StatusRepository,
}

impl<'a> CaptureTaskService<'a> {
    #[must_use]
    pub fn new(
        tasks: &'a dyn TaskRepository,
        lists: &'a dyn ListRepository,
        statuses: &'a dyn StatusRepository,
    ) -> Self {
        Self {
            tasks,
            lists,
            statuses,
        }
    }

    pub async fn capture_inbox_task(
        &self,
        request: CaptureTaskRequest,
    ) -> AppResult<CaptureTaskResult> {
        let title = request.title.trim();
        if title.is_empty() {
            return Err(AppError::EmptyTaskTitle);
        }

        let inbox = self
            .lists
            .list_system()
            .await?
            .into_iter()
            .find(|list| list.kind == ListKind::Inbox)
            .ok_or(AppError::MissingDefaultInbox)?;
        let status_id = default_task_status_id(self.statuses).await?;
        let now = OffsetDateTime::now_utc();
        let task = Task {
            id: TaskId::new(),
            title: title.to_string(),
            description: request.description,
            project_id: None,
            list_id: Some(inbox.id),
            status_id,
            due_date: request.due_date,
            start_date: None,
            estimated_minutes: request.estimated_minutes,
            cost_points: None,
            dependencies: Vec::new(),
            milestone_id: None,
            created_at: now,
            updated_at: now,
            deleted_at: None,
        };

        self.tasks.insert(task.clone()).await?;
        Ok(CaptureTaskResult { task })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanTodayRequest {
    pub target_date: Date,
    pub availability: Vec<AvailabilityWindow>,
    pub busy_blocks: Vec<BusyBlock>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanTodayResult {
    pub target_date: Date,
    pub output: SchedulingOutput,
}

pub struct PlanTodayService<'a> {
    tasks: &'a dyn TaskRepository,
    statuses: &'a dyn StatusRepository,
    scheduler: GreedyScheduler,
}

impl<'a> PlanTodayService<'a> {
    #[must_use]
    pub fn new(tasks: &'a dyn TaskRepository, statuses: &'a dyn StatusRepository) -> Self {
        Self {
            tasks,
            statuses,
            scheduler: GreedyScheduler::default(),
        }
    }

    #[must_use]
    pub fn with_scheduler(
        tasks: &'a dyn TaskRepository,
        statuses: &'a dyn StatusRepository,
        scheduler: GreedyScheduler,
    ) -> Self {
        Self {
            tasks,
            statuses,
            scheduler,
        }
    }

    pub async fn plan_today(&self, request: PlanTodayRequest) -> AppResult<PlanTodayResult> {
        let all_tasks = self.tasks.list_all().await?;
        let tasks = filter_today_candidates(all_tasks, request.target_date);
        let status_groups = self.statuses.list_groups().await?;
        let statuses = self.statuses_for_tasks(&tasks).await?;

        let output = if request.availability.is_empty() {
            SchedulingOutput {
                blocks: Vec::new(),
                unscheduled: tasks.into_iter().map(|task| task.id).collect(),
                issues: vec![ScheduleIssue::NoAvailability],
            }
        } else {
            self.scheduler.plan(SchedulingInput {
                tasks,
                statuses,
                status_groups,
                availability: request.availability,
                busy_blocks: request.busy_blocks,
            })
        };

        Ok(PlanTodayResult {
            target_date: request.target_date,
            output,
        })
    }

    async fn statuses_for_tasks(&self, tasks: &[Task]) -> AppResult<Vec<Status>> {
        let mut statuses = self.statuses.list_statuses_for_project(None).await?;
        let mut project_ids = HashSet::new();

        for task in tasks {
            if let Some(project_id) = &task.project_id {
                project_ids.insert(project_id.clone());
            }
        }

        for project_id in project_ids {
            statuses.extend(
                self.statuses
                    .list_statuses_for_project(Some(project_id))
                    .await?,
            );
        }

        Ok(statuses)
    }
}

pub struct SchedulePlanStoreService<'a> {
    schedule_blocks: &'a dyn ScheduleBlockRepository,
}

impl<'a> SchedulePlanStoreService<'a> {
    #[must_use]
    pub fn new(schedule_blocks: &'a dyn ScheduleBlockRepository) -> Self {
        Self { schedule_blocks }
    }

    pub async fn save_proposed_plan(
        &self,
        plan: &PlanTodayResult,
    ) -> AppResult<Vec<ScheduleBlock>> {
        let now = OffsetDateTime::now_utc();
        let blocks = plan
            .output
            .blocks
            .iter()
            .map(|block| ScheduleBlock {
                id: ScheduleBlockId::new(),
                task_id: Some(block.task_id.clone()),
                title_snapshot: Some(block.title.clone()),
                start_at: block.window.start,
                end_at: block.window.end,
                block_type: ScheduleBlockType::Task,
                state: ScheduleBlockState::Proposed,
                locked: false,
                source: ScheduleBlockSource::Scheduler,
                required_minutes: Some(block.required_minutes),
                created_at: now,
                updated_at: now,
            })
            .collect::<Vec<_>>();

        self.schedule_blocks
            .replace_proposed_for_day(plan.target_date, blocks.clone())
            .await?;

        Ok(blocks)
    }

    pub async fn list_for_day(&self, day: Date) -> AppResult<Vec<ScheduleBlock>> {
        self.schedule_blocks
            .list_for_day(day)
            .await
            .map_err(Into::into)
    }
}

async fn default_task_status_id(statuses: &dyn StatusRepository) -> AppResult<StatusId> {
    let groups = statuses.list_groups().await?;
    let group_kinds = groups
        .into_iter()
        .map(|group| (group.id, group.kind))
        .collect::<HashMap<_, _>>();
    let statuses = statuses.list_statuses_for_project(None).await?;

    statuses
        .iter()
        .find(|status| {
            matches!(
                group_kinds.get(&status.group_id),
                Some(StatusGroupKind::NotStarted)
            )
        })
        .or_else(|| {
            statuses.iter().find(|status| {
                !matches!(
                    group_kinds.get(&status.group_id),
                    Some(StatusGroupKind::Done)
                )
            })
        })
        .map(|status| status.id.clone())
        .ok_or(AppError::MissingDefaultStatus)
}

fn filter_today_candidates(tasks: Vec<Task>, target_date: Date) -> Vec<Task> {
    tasks
        .into_iter()
        .filter(|task| task.deleted_at.is_none())
        .filter(|task| match (task.due_date, task.start_date) {
            (None, None) => true,
            (Some(due_date), _) if due_date <= target_date => true,
            (_, Some(start_date)) if start_date <= target_date => true,
            _ => false,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Mutex;
    use time::macros::{date, datetime};

    #[derive(Default)]
    struct MemoryTaskRepository {
        tasks: Vec<Task>,
    }

    #[async_trait]
    impl TaskRepository for MemoryTaskRepository {
        async fn insert(&self, _task: Task) -> CoreResult<()> {
            unimplemented!("not needed")
        }

        async fn find(&self, _id: TaskId) -> CoreResult<Option<Task>> {
            unimplemented!("not needed")
        }

        async fn update(&self, _task: Task) -> CoreResult<()> {
            unimplemented!("not needed")
        }

        async fn list_all(&self) -> CoreResult<Vec<Task>> {
            Ok(self.tasks.clone())
        }

        async fn list_by_project(&self, _project_id: ProjectId) -> CoreResult<Vec<Task>> {
            unimplemented!("not needed")
        }

        async fn list_by_list(&self, _list_id: ListId) -> CoreResult<Vec<Task>> {
            unimplemented!("not needed")
        }

        async fn soft_delete(
            &self,
            _id: TaskId,
            _deleted_at: time::OffsetDateTime,
        ) -> CoreResult<()> {
            unimplemented!("not needed")
        }
    }

    #[derive(Clone)]
    struct MemoryStatusRepository {
        groups: Vec<StatusGroup>,
        statuses: Vec<Status>,
    }

    #[async_trait]
    impl StatusRepository for MemoryStatusRepository {
        async fn insert_group(&self, _group: StatusGroup) -> CoreResult<()> {
            unimplemented!("not needed")
        }

        async fn insert_status(&self, _status: Status) -> CoreResult<()> {
            unimplemented!("not needed")
        }

        async fn list_groups(&self) -> CoreResult<Vec<StatusGroup>> {
            Ok(self.groups.clone())
        }

        async fn list_statuses_for_project(
            &self,
            project_id: Option<ProjectId>,
        ) -> CoreResult<Vec<Status>> {
            Ok(self
                .statuses
                .iter()
                .filter(|status| status.project_id == project_id)
                .cloned()
                .collect())
        }
    }

    struct CapturingTaskRepository {
        tasks: Mutex<Vec<Task>>,
    }

    #[async_trait]
    impl TaskRepository for CapturingTaskRepository {
        async fn insert(&self, task: Task) -> CoreResult<()> {
            self.tasks.lock().unwrap().push(task);
            Ok(())
        }

        async fn find(&self, id: TaskId) -> CoreResult<Option<Task>> {
            Ok(self
                .tasks
                .lock()
                .unwrap()
                .iter()
                .find(|task| task.id == id)
                .cloned())
        }

        async fn update(&self, _task: Task) -> CoreResult<()> {
            unimplemented!("not needed")
        }

        async fn list_all(&self) -> CoreResult<Vec<Task>> {
            Ok(self.tasks.lock().unwrap().clone())
        }

        async fn list_by_project(&self, _project_id: ProjectId) -> CoreResult<Vec<Task>> {
            unimplemented!("not needed")
        }

        async fn list_by_list(&self, _list_id: ListId) -> CoreResult<Vec<Task>> {
            unimplemented!("not needed")
        }

        async fn soft_delete(
            &self,
            _id: TaskId,
            _deleted_at: time::OffsetDateTime,
        ) -> CoreResult<()> {
            unimplemented!("not needed")
        }
    }

    struct MemoryListRepository {
        lists: Vec<List>,
    }

    #[async_trait]
    impl ListRepository for MemoryListRepository {
        async fn insert(&self, _list: List) -> CoreResult<()> {
            unimplemented!("not needed")
        }

        async fn find(&self, _id: ListId) -> CoreResult<Option<List>> {
            unimplemented!("not needed")
        }

        async fn list_by_project(&self, _project_id: ProjectId) -> CoreResult<Vec<List>> {
            unimplemented!("not needed")
        }

        async fn list_system(&self) -> CoreResult<Vec<List>> {
            Ok(self.lists.clone())
        }

        async fn update(&self, _list: List) -> CoreResult<()> {
            unimplemented!("not needed")
        }
    }

    struct MemoryScheduleBlockRepository {
        blocks: Mutex<Vec<ScheduleBlock>>,
    }

    #[async_trait]
    impl ScheduleBlockRepository for MemoryScheduleBlockRepository {
        async fn insert(&self, block: ScheduleBlock) -> CoreResult<()> {
            self.blocks.lock().unwrap().push(block);
            Ok(())
        }

        async fn find(&self, id: ScheduleBlockId) -> CoreResult<Option<ScheduleBlock>> {
            Ok(self
                .blocks
                .lock()
                .unwrap()
                .iter()
                .find(|block| block.id == id)
                .cloned())
        }

        async fn list_for_day(&self, _day: Date) -> CoreResult<Vec<ScheduleBlock>> {
            Ok(self.blocks.lock().unwrap().clone())
        }

        async fn replace_proposed_for_day(
            &self,
            _day: Date,
            blocks: Vec<ScheduleBlock>,
        ) -> CoreResult<()> {
            let mut stored = self.blocks.lock().unwrap();
            stored.retain(|block| {
                !(block.source == ScheduleBlockSource::Scheduler
                    && block.state == ScheduleBlockState::Proposed)
            });
            stored.extend(blocks);
            Ok(())
        }
    }

    fn status_catalog() -> (MemoryStatusRepository, StatusId, StatusId) {
        let todo_group = StatusGroup {
            id: StatusGroupId::new(),
            name: "Not started".into(),
            kind: StatusGroupKind::NotStarted,
        };
        let done_group = StatusGroup {
            id: StatusGroupId::new(),
            name: "Done".into(),
            kind: StatusGroupKind::Done,
        };
        let todo = Status {
            id: StatusId::new(),
            project_id: None,
            name: "To do".into(),
            group_id: todo_group.id.clone(),
            order: 0,
        };
        let done = Status {
            id: StatusId::new(),
            project_id: None,
            name: "Done".into(),
            group_id: done_group.id.clone(),
            order: 1,
        };

        (
            MemoryStatusRepository {
                groups: vec![todo_group, done_group],
                statuses: vec![todo.clone(), done.clone()],
            },
            todo.id,
            done.id,
        )
    }

    fn task(status_id: StatusId, title: &str, due_date: Option<Date>) -> Task {
        Task {
            id: TaskId::new(),
            title: title.into(),
            description: None,
            project_id: None,
            list_id: None,
            status_id,
            due_date,
            start_date: None,
            estimated_minutes: Some(30),
            cost_points: None,
            dependencies: vec![],
            milestone_id: None,
            created_at: datetime!(2026-06-25 00:00 UTC),
            updated_at: datetime!(2026-06-25 00:00 UTC),
            deleted_at: None,
        }
    }

    fn availability() -> Vec<AvailabilityWindow> {
        vec![AvailabilityWindow {
            window: TimeWindow::new(
                datetime!(2026-06-25 09:00 UTC),
                datetime!(2026-06-25 10:00 UTC),
            ),
        }]
    }

    fn inbox_list() -> List {
        List {
            id: ListId::new(),
            project_id: None,
            name: "Inbox".into(),
            is_system: true,
            kind: ListKind::Inbox,
            view_type: ListViewType::List,
            order: 0,
        }
    }

    #[tokio::test]
    async fn plans_today_from_repositories() {
        let (statuses, todo_status, _) = status_catalog();
        let tasks = MemoryTaskRepository {
            tasks: vec![task(todo_status, "Write code", Some(date!(2026 - 06 - 25)))],
        };
        let service = PlanTodayService::new(&tasks, &statuses);

        let result = service
            .plan_today(PlanTodayRequest {
                target_date: date!(2026 - 06 - 25),
                availability: availability(),
                busy_blocks: vec![],
            })
            .await
            .unwrap();

        assert_eq!(result.output.blocks.len(), 1);
        assert_eq!(result.output.blocks[0].title, "Write code");
    }

    #[tokio::test]
    async fn excludes_future_tasks_before_scheduler() {
        let (statuses, todo_status, _) = status_catalog();
        let tasks = MemoryTaskRepository {
            tasks: vec![task(
                todo_status,
                "Future task",
                Some(date!(2026 - 06 - 26)),
            )],
        };
        let service = PlanTodayService::new(&tasks, &statuses);

        let result = service
            .plan_today(PlanTodayRequest {
                target_date: date!(2026 - 06 - 25),
                availability: availability(),
                busy_blocks: vec![],
            })
            .await
            .unwrap();

        assert!(result.output.blocks.is_empty());
        assert!(result.output.unscheduled.is_empty());
    }

    #[tokio::test]
    async fn excludes_done_tasks_through_scheduler_status_mapping() {
        let (statuses, _, done_status) = status_catalog();
        let tasks = MemoryTaskRepository {
            tasks: vec![task(done_status, "Done task", Some(date!(2026 - 06 - 25)))],
        };
        let service = PlanTodayService::new(&tasks, &statuses);

        let result = service
            .plan_today(PlanTodayRequest {
                target_date: date!(2026 - 06 - 25),
                availability: availability(),
                busy_blocks: vec![],
            })
            .await
            .unwrap();

        assert!(result.output.blocks.is_empty());
        assert!(result.output.unscheduled.is_empty());
    }

    #[tokio::test]
    async fn captures_task_into_inbox_with_default_status() {
        let (statuses, todo_status, _) = status_catalog();
        let tasks = CapturingTaskRepository {
            tasks: Mutex::new(Vec::new()),
        };
        let lists = MemoryListRepository {
            lists: vec![inbox_list()],
        };
        let service = CaptureTaskService::new(&tasks, &lists, &statuses);

        let result = service
            .capture_inbox_task(CaptureTaskRequest {
                title: "  Write first task  ".into(),
                description: None,
                due_date: Some(date!(2026 - 06 - 25)),
                estimated_minutes: Some(45),
            })
            .await
            .unwrap();

        assert_eq!(result.task.title, "Write first task");
        assert_eq!(result.task.status_id, todo_status);
        assert_eq!(tasks.list_all().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn rejects_empty_capture_title() {
        let (statuses, _, _) = status_catalog();
        let tasks = CapturingTaskRepository {
            tasks: Mutex::new(Vec::new()),
        };
        let lists = MemoryListRepository {
            lists: vec![inbox_list()],
        };
        let service = CaptureTaskService::new(&tasks, &lists, &statuses);

        let err = service
            .capture_inbox_task(CaptureTaskRequest {
                title: "   ".into(),
                description: None,
                due_date: None,
                estimated_minutes: None,
            })
            .await
            .unwrap_err();

        assert!(matches!(err, AppError::EmptyTaskTitle));
    }

    #[tokio::test]
    async fn saves_proposed_plan_as_schedule_blocks() {
        let schedule_blocks = MemoryScheduleBlockRepository {
            blocks: Mutex::new(Vec::new()),
        };
        let task_id = TaskId::new();
        let plan = PlanTodayResult {
            target_date: date!(2026 - 06 - 25),
            output: SchedulingOutput {
                blocks: vec![ProposedScheduleBlock {
                    task_id: task_id.clone(),
                    title: "Write code".into(),
                    window: TimeWindow::new(
                        datetime!(2026-06-25 09:00 UTC),
                        datetime!(2026-06-25 09:30 UTC),
                    ),
                    required_minutes: 30,
                }],
                unscheduled: Vec::new(),
                issues: Vec::new(),
            },
        };
        let service = SchedulePlanStoreService::new(&schedule_blocks);

        let saved = service.save_proposed_plan(&plan).await.unwrap();

        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].task_id, Some(task_id));
        assert_eq!(saved[0].state, ScheduleBlockState::Proposed);
        assert_eq!(
            service.list_for_day(plan.target_date).await.unwrap().len(),
            1
        );
    }
}
