//! Application services that connect Mnema domain repositories to pure logic.

use std::collections::HashSet;

use mnema_core::prelude::*;
use mnema_scheduler::{ScheduleIssue, SchedulingOutput};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::Date;

pub use mnema_scheduler::{
    AvailabilityWindow, BusyBlock, BusyBlockSource, GreedyScheduler, ProposedScheduleBlock,
    SchedulerConfig, SchedulingInput, TimeWindow,
};

#[derive(Debug, Error)]
pub enum AppError {
    #[error("repository error: {0}")]
    Repository(String),
}

impl From<CoreError> for AppError {
    fn from(value: CoreError) -> Self {
        Self::Repository(value.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;

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
}
