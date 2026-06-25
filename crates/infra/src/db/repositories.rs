use anyhow::Result;
use mnema_core::prelude::*;
use sqlx::{PgPool, Row, SqlitePool};
use time::format_description::well_known::Rfc3339;
use time::{Date, Duration, OffsetDateTime};

fn status_group_kind_to_str(kind: &StatusGroupKind) -> &'static str {
    match kind {
        StatusGroupKind::NotStarted => "NOT_STARTED",
        StatusGroupKind::InProgress => "IN_PROGRESS",
        StatusGroupKind::Pending => "PENDING",
        StatusGroupKind::Done => "DONE",
    }
}

fn status_kind_from_str(s: &str) -> StatusGroupKind {
    match s {
        "IN_PROGRESS" => StatusGroupKind::InProgress,
        "PENDING" => StatusGroupKind::Pending,
        "DONE" => StatusGroupKind::Done,
        _ => StatusGroupKind::NotStarted,
    }
}

fn list_kind_to_str(kind: &ListKind) -> &'static str {
    match kind {
        ListKind::Inbox => "INBOX",
        ListKind::Personal => "PERSONAL",
        ListKind::Project => "PROJECT",
    }
}

fn list_view_to_str(view: &ListViewType) -> &'static str {
    match view {
        ListViewType::Board => "BOARD",
        ListViewType::Calendar => "CALENDAR",
        ListViewType::Gantt => "GANTT",
        ListViewType::List => "LIST",
    }
}

fn list_kind_from_str(s: &str) -> ListKind {
    match s {
        "INBOX" => ListKind::Inbox,
        "PERSONAL" => ListKind::Personal,
        _ => ListKind::Project,
    }
}

fn list_view_from_str(s: &str) -> ListViewType {
    match s {
        "BOARD" => ListViewType::Board,
        "CALENDAR" => ListViewType::Calendar,
        "GANTT" => ListViewType::Gantt,
        _ => ListViewType::List,
    }
}

fn milestone_status_to_str(status: &MilestoneStatus) -> &'static str {
    match status {
        MilestoneStatus::NotDone => "NOT_DONE",
        MilestoneStatus::Overdue => "OVERDUE",
        MilestoneStatus::Done => "DONE",
    }
}

fn milestone_status_from_str(s: &str) -> MilestoneStatus {
    match s {
        "DONE" => MilestoneStatus::Done,
        "OVERDUE" => MilestoneStatus::Overdue,
        _ => MilestoneStatus::NotDone,
    }
}

fn to_rfc3339(dt: OffsetDateTime) -> Result<String> {
    dt.format(&Rfc3339).map_err(Into::into)
}

fn from_rfc3339(s: &str) -> Result<OffsetDateTime> {
    OffsetDateTime::parse(s, &Rfc3339).map_err(Into::into)
}

fn date_to_string(date: Date) -> String {
    date.to_string()
}

fn date_from_string(s: &str) -> Result<Date> {
    Date::parse(s, time::macros::format_description!("[year]-[month]-[day]")).map_err(Into::into)
}

fn llm_provider_to_string(provider: &LlmProvider) -> String {
    match provider {
        LlmProvider::Local => "LOCAL".to_string(),
        LlmProvider::OpenAiCompatible => "OPEN_AI_COMPATIBLE".to_string(),
        LlmProvider::Other(s) => format!("OTHER:{s}"),
    }
}

fn llm_provider_from_string(provider: &str) -> LlmProvider {
    if provider == "LOCAL" {
        LlmProvider::Local
    } else if provider == "OPEN_AI_COMPATIBLE" {
        LlmProvider::OpenAiCompatible
    } else if let Some(rest) = provider.strip_prefix("OTHER:") {
        LlmProvider::Other(rest.to_string())
    } else {
        LlmProvider::Local
    }
}

fn schedule_block_type_to_str(block_type: &ScheduleBlockType) -> &'static str {
    match block_type {
        ScheduleBlockType::Task => "TASK",
        ScheduleBlockType::Habit => "HABIT",
        ScheduleBlockType::ExternalEvent => "EXTERNAL_EVENT",
        ScheduleBlockType::Buffer => "BUFFER",
    }
}

fn schedule_block_type_from_str(value: &str) -> ScheduleBlockType {
    match value {
        "HABIT" => ScheduleBlockType::Habit,
        "EXTERNAL_EVENT" => ScheduleBlockType::ExternalEvent,
        "BUFFER" => ScheduleBlockType::Buffer,
        _ => ScheduleBlockType::Task,
    }
}

fn schedule_block_state_to_str(state: &ScheduleBlockState) -> &'static str {
    match state {
        ScheduleBlockState::Proposed => "PROPOSED",
        ScheduleBlockState::Scheduled => "SCHEDULED",
        ScheduleBlockState::Active => "ACTIVE",
        ScheduleBlockState::Done => "DONE",
        ScheduleBlockState::Missed => "MISSED",
        ScheduleBlockState::Cancelled => "CANCELLED",
    }
}

fn schedule_block_state_from_str(value: &str) -> ScheduleBlockState {
    match value {
        "SCHEDULED" => ScheduleBlockState::Scheduled,
        "ACTIVE" => ScheduleBlockState::Active,
        "DONE" => ScheduleBlockState::Done,
        "MISSED" => ScheduleBlockState::Missed,
        "CANCELLED" => ScheduleBlockState::Cancelled,
        _ => ScheduleBlockState::Proposed,
    }
}

fn schedule_block_source_to_str(source: &ScheduleBlockSource) -> &'static str {
    match source {
        ScheduleBlockSource::Scheduler => "SCHEDULER",
        ScheduleBlockSource::Manual => "MANUAL",
        ScheduleBlockSource::ExternalCalendar => "EXTERNAL_CALENDAR",
    }
}

fn schedule_block_source_from_str(value: &str) -> ScheduleBlockSource {
    match value {
        "MANUAL" => ScheduleBlockSource::Manual,
        "EXTERNAL_CALENDAR" => ScheduleBlockSource::ExternalCalendar,
        _ => ScheduleBlockSource::Scheduler,
    }
}

fn map_storage_err(e: impl ToString) -> CoreError {
    CoreError::Storage(e.to_string())
}

fn day_bounds(day: Date) -> Result<(OffsetDateTime, OffsetDateTime)> {
    let start = day.with_hms(0, 0, 0)?.assume_utc();
    Ok((start, start + Duration::days(1)))
}

#[derive(Clone)]
pub struct PostgresTaskRepository {
    pool: PgPool,
}

impl PostgresTaskRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl TaskRepository for PostgresTaskRepository {
    async fn insert(&self, task: Task) -> CoreResult<()> {
        let dependencies = serde_json::to_value(&task.dependencies).map_err(map_storage_err)?;
        sqlx::query(
            r#"
            INSERT INTO tasks (
                id, title, description, project_id, list_id, status_id,
                due_date, start_date, estimated_minutes, cost_points, dependencies,
                milestone_id, created_at, updated_at, deleted_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
        "#,
        )
        .bind(task.id.0)
        .bind(task.title)
        .bind(task.description)
        .bind(task.project_id.map(|p| p.0))
        .bind(task.list_id.map(|l| l.0))
        .bind(task.status_id.0)
        .bind(task.due_date)
        .bind(task.start_date)
        .bind(task.estimated_minutes.map(i64::from))
        .bind(task.cost_points.map(i64::from))
        .bind(dependencies)
        .bind(task.milestone_id.map(|m| m.0))
        .bind(task.created_at)
        .bind(task.updated_at)
        .bind(task.deleted_at)
        .execute(&self.pool)
        .await
        .map_err(map_storage_err)?;
        Ok(())
    }

    async fn find(&self, id: TaskId) -> CoreResult<Option<Task>> {
        let row = sqlx::query(
            r#"
            SELECT *
            FROM tasks
            WHERE id = $1
        "#,
        )
        .bind(id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_storage_err)?;

        row.map(row_to_task).transpose().map_err(map_storage_err)
    }

    async fn update(&self, task: Task) -> CoreResult<()> {
        let dependencies = serde_json::to_value(&task.dependencies).map_err(map_storage_err)?;
        let rows = sqlx::query(
            r#"
            UPDATE tasks SET
                title = $1,
                description = $2,
                project_id = $3,
                list_id = $4,
                status_id = $5,
                due_date = $6,
                start_date = $7,
                estimated_minutes = $8,
                cost_points = $9,
                dependencies = $10,
                milestone_id = $11,
                created_at = $12,
                updated_at = $13,
                deleted_at = $14
            WHERE id = $15
        "#,
        )
        .bind(task.title)
        .bind(task.description)
        .bind(task.project_id.map(|p| p.0))
        .bind(task.list_id.map(|l| l.0))
        .bind(task.status_id.0)
        .bind(task.due_date)
        .bind(task.start_date)
        .bind(task.estimated_minutes.map(i64::from))
        .bind(task.cost_points.map(i64::from))
        .bind(dependencies)
        .bind(task.milestone_id.map(|m| m.0))
        .bind(task.created_at)
        .bind(task.updated_at)
        .bind(task.deleted_at)
        .bind(task.id.0)
        .execute(&self.pool)
        .await
        .map_err(map_storage_err)?;
        if rows.rows_affected() == 0 {
            return Err(CoreError::NotFound);
        }
        Ok(())
    }

    async fn list_all(&self) -> CoreResult<Vec<Task>> {
        let rows = sqlx::query(
            r#"
            SELECT *
            FROM tasks
            ORDER BY created_at, title
        "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(map_storage_err)?;

        rows.into_iter()
            .map(row_to_task)
            .map(|r| r.map_err(map_storage_err))
            .collect()
    }

    async fn list_by_project(&self, project_id: ProjectId) -> CoreResult<Vec<Task>> {
        let rows = sqlx::query(
            r#"
            SELECT *
            FROM tasks
            WHERE project_id = $1
        "#,
        )
        .bind(project_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(map_storage_err)?;

        rows.into_iter()
            .map(row_to_task)
            .map(|r| r.map_err(map_storage_err))
            .collect()
    }

    async fn list_by_list(&self, list_id: ListId) -> CoreResult<Vec<Task>> {
        let rows = sqlx::query(
            r#"
            SELECT *
            FROM tasks
            WHERE list_id = $1
        "#,
        )
        .bind(list_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(map_storage_err)?;

        rows.into_iter()
            .map(row_to_task)
            .map(|r| r.map_err(map_storage_err))
            .collect()
    }

    async fn soft_delete(&self, id: TaskId, deleted_at: time::OffsetDateTime) -> CoreResult<()> {
        let res = sqlx::query("UPDATE tasks SET deleted_at = $1 WHERE id = $2")
            .bind(deleted_at)
            .bind(id.0)
            .execute(&self.pool)
            .await
            .map_err(map_storage_err)?;
        if res.rows_affected() == 0 {
            return Err(CoreError::NotFound);
        }
        Ok(())
    }
}

fn row_to_task(row: sqlx::postgres::PgRow) -> Result<Task> {
    let dependencies_value: serde_json::Value = row.try_get("dependencies")?;
    let dependencies: Vec<TaskId> = serde_json::from_value(dependencies_value)?;

    Ok(Task {
        id: TaskId::from(row.try_get::<uuid::Uuid, _>("id")?),
        title: row.try_get("title")?,
        description: row.try_get("description")?,
        project_id: row
            .try_get::<Option<uuid::Uuid>, _>("project_id")?
            .map(ProjectId::from),
        list_id: row
            .try_get::<Option<uuid::Uuid>, _>("list_id")?
            .map(ListId::from),
        status_id: StatusId::from(row.try_get::<uuid::Uuid, _>("status_id")?),
        due_date: row.try_get("due_date")?,
        start_date: row.try_get("start_date")?,
        estimated_minutes: row
            .try_get::<Option<i64>, _>("estimated_minutes")?
            .map(|v| v as u32),
        cost_points: row
            .try_get::<Option<i64>, _>("cost_points")?
            .map(|v| v as u32),
        dependencies,
        milestone_id: row
            .try_get::<Option<uuid::Uuid>, _>("milestone_id")?
            .map(MilestoneId::from),
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        deleted_at: row.try_get("deleted_at")?,
    })
}

#[derive(Clone)]
pub struct PostgresProjectRepository {
    pool: PgPool,
}

impl PostgresProjectRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl ProjectRepository for PostgresProjectRepository {
    async fn insert(&self, project: Project) -> CoreResult<()> {
        sqlx::query(
            r#"
            INSERT INTO projects (
                id, title, description, start_date, end_date, default_status_set_id, archived_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
        )
        .bind(project.id.0)
        .bind(project.title)
        .bind(project.description)
        .bind(project.start_date)
        .bind(project.end_date)
        .bind(project.default_status_set_id.map(|id| id.0))
        .bind(project.archived_at)
        .execute(&self.pool)
        .await
        .map_err(map_storage_err)?;
        Ok(())
    }

    async fn find(&self, id: ProjectId) -> CoreResult<Option<Project>> {
        let row = sqlx::query("SELECT * FROM projects WHERE id = $1")
            .bind(id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(map_storage_err)?;

        row.map(row_to_project).transpose().map_err(map_storage_err)
    }

    async fn list_all(&self) -> CoreResult<Vec<Project>> {
        let rows = sqlx::query("SELECT * FROM projects ORDER BY title")
            .fetch_all(&self.pool)
            .await
            .map_err(map_storage_err)?;

        rows.into_iter()
            .map(row_to_project)
            .map(|r| r.map_err(map_storage_err))
            .collect()
    }

    async fn update(&self, project: Project) -> CoreResult<()> {
        let rows = sqlx::query(
            r#"
            UPDATE projects SET
                title = $1,
                description = $2,
                start_date = $3,
                end_date = $4,
                default_status_set_id = $5,
                archived_at = $6
            WHERE id = $7
        "#,
        )
        .bind(project.title)
        .bind(project.description)
        .bind(project.start_date)
        .bind(project.end_date)
        .bind(project.default_status_set_id.map(|id| id.0))
        .bind(project.archived_at)
        .bind(project.id.0)
        .execute(&self.pool)
        .await
        .map_err(map_storage_err)?;
        if rows.rows_affected() == 0 {
            return Err(CoreError::NotFound);
        }
        Ok(())
    }
}

fn row_to_project(row: sqlx::postgres::PgRow) -> Result<Project> {
    Ok(Project {
        id: ProjectId::from(row.try_get::<uuid::Uuid, _>("id")?),
        title: row.try_get("title")?,
        description: row.try_get("description")?,
        start_date: row.try_get("start_date")?,
        end_date: row.try_get("end_date")?,
        default_status_set_id: row
            .try_get::<Option<uuid::Uuid>, _>("default_status_set_id")?
            .map(StatusGroupId::from),
        archived_at: row.try_get("archived_at")?,
    })
}

#[derive(Clone)]
pub struct PostgresListRepository {
    pool: PgPool,
}

impl PostgresListRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl ListRepository for PostgresListRepository {
    async fn insert(&self, list: List) -> CoreResult<()> {
        sqlx::query(
            r#"
            INSERT INTO lists (id, project_id, name, is_system, kind, view_type, "order")
            VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
        )
        .bind(list.id.0)
        .bind(list.project_id.map(|p| p.0))
        .bind(list.name)
        .bind(list.is_system)
        .bind(list_kind_to_str(&list.kind))
        .bind(list_view_to_str(&list.view_type))
        .bind(list.order)
        .execute(&self.pool)
        .await
        .map_err(map_storage_err)?;
        Ok(())
    }

    async fn find(&self, id: ListId) -> CoreResult<Option<List>> {
        let row = sqlx::query("SELECT * FROM lists WHERE id = $1")
            .bind(id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(map_storage_err)?;

        row.map(row_to_list).transpose().map_err(map_storage_err)
    }

    async fn list_by_project(&self, project_id: ProjectId) -> CoreResult<Vec<List>> {
        let rows = sqlx::query(r#"SELECT * FROM lists WHERE project_id = $1 ORDER BY "order""#)
            .bind(project_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(map_storage_err)?;
        rows.into_iter()
            .map(row_to_list)
            .map(|r| r.map_err(map_storage_err))
            .collect()
    }

    async fn list_system(&self) -> CoreResult<Vec<List>> {
        let rows = sqlx::query(r#"SELECT * FROM lists WHERE is_system = TRUE ORDER BY "order""#)
            .fetch_all(&self.pool)
            .await
            .map_err(map_storage_err)?;
        rows.into_iter()
            .map(row_to_list)
            .map(|r| r.map_err(map_storage_err))
            .collect()
    }

    async fn update(&self, list: List) -> CoreResult<()> {
        let rows = sqlx::query(
            r#"
            UPDATE lists SET
                project_id = $1,
                name = $2,
                is_system = $3,
                kind = $4,
                view_type = $5,
                "order" = $6
            WHERE id = $7
        "#,
        )
        .bind(list.project_id.map(|p| p.0))
        .bind(list.name)
        .bind(list.is_system)
        .bind(list_kind_to_str(&list.kind))
        .bind(list_view_to_str(&list.view_type))
        .bind(list.order)
        .bind(list.id.0)
        .execute(&self.pool)
        .await
        .map_err(map_storage_err)?;
        if rows.rows_affected() == 0 {
            return Err(CoreError::NotFound);
        }
        Ok(())
    }
}

fn row_to_list(row: sqlx::postgres::PgRow) -> Result<List> {
    let kind_str: String = row.try_get("kind")?;
    let view_str: String = row.try_get("view_type")?;

    Ok(List {
        id: ListId::from(row.try_get::<uuid::Uuid, _>("id")?),
        project_id: row
            .try_get::<Option<uuid::Uuid>, _>("project_id")?
            .map(ProjectId::from),
        name: row.try_get("name")?,
        is_system: row.try_get("is_system")?,
        kind: list_kind_from_str(&kind_str),
        view_type: list_view_from_str(&view_str),
        order: row.try_get("order")?,
    })
}

#[derive(Clone)]
pub struct PostgresMilestoneRepository {
    pool: PgPool,
}

impl PostgresMilestoneRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl MilestoneRepository for PostgresMilestoneRepository {
    async fn insert(&self, milestone: Milestone) -> CoreResult<()> {
        let deps = serde_json::to_value(&milestone.dependency_task_ids).map_err(map_storage_err)?;
        sqlx::query(
            r#"
            INSERT INTO milestones (
                id, project_id, title, description, target_date, status,
                dependency_task_ids, created_at, updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        "#,
        )
        .bind(milestone.id.0)
        .bind(milestone.project_id.0)
        .bind(milestone.title)
        .bind(milestone.description)
        .bind(milestone.target_date)
        .bind(milestone_status_to_str(&milestone.status))
        .bind(deps)
        .bind(milestone.created_at)
        .bind(milestone.updated_at)
        .execute(&self.pool)
        .await
        .map_err(map_storage_err)?;
        Ok(())
    }

    async fn find(&self, id: MilestoneId) -> CoreResult<Option<Milestone>> {
        let row = sqlx::query("SELECT * FROM milestones WHERE id = $1")
            .bind(id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(map_storage_err)?;

        row.map(row_to_milestone)
            .transpose()
            .map_err(map_storage_err)
    }

    async fn list_by_project(&self, project_id: ProjectId) -> CoreResult<Vec<Milestone>> {
        let rows =
            sqlx::query("SELECT * FROM milestones WHERE project_id = $1 ORDER BY target_date")
                .bind(project_id.0)
                .fetch_all(&self.pool)
                .await
                .map_err(map_storage_err)?;
        rows.into_iter()
            .map(row_to_milestone)
            .map(|r| r.map_err(map_storage_err))
            .collect()
    }

    async fn update(&self, milestone: Milestone) -> CoreResult<()> {
        let deps = serde_json::to_value(&milestone.dependency_task_ids).map_err(map_storage_err)?;
        let rows = sqlx::query(
            r#"
            UPDATE milestones SET
                project_id = $1,
                title = $2,
                description = $3,
                target_date = $4,
                status = $5,
                dependency_task_ids = $6,
                created_at = $7,
                updated_at = $8
            WHERE id = $9
        "#,
        )
        .bind(milestone.project_id.0)
        .bind(milestone.title)
        .bind(milestone.description)
        .bind(milestone.target_date)
        .bind(milestone_status_to_str(&milestone.status))
        .bind(deps)
        .bind(milestone.created_at)
        .bind(milestone.updated_at)
        .bind(milestone.id.0)
        .execute(&self.pool)
        .await
        .map_err(map_storage_err)?;
        if rows.rows_affected() == 0 {
            return Err(CoreError::NotFound);
        }
        Ok(())
    }
}

fn row_to_milestone(row: sqlx::postgres::PgRow) -> Result<Milestone> {
    let deps_value: serde_json::Value = row.try_get("dependency_task_ids")?;
    let dependency_task_ids: Vec<TaskId> = serde_json::from_value(deps_value)?;
    let status_str: String = row.try_get("status")?;

    Ok(Milestone {
        id: MilestoneId::from(row.try_get::<uuid::Uuid, _>("id")?),
        project_id: ProjectId::from(row.try_get::<uuid::Uuid, _>("project_id")?),
        title: row.try_get("title")?,
        description: row.try_get("description")?,
        target_date: row.try_get("target_date")?,
        status: milestone_status_from_str(&status_str),
        dependency_task_ids,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

#[derive(Clone)]
pub struct PostgresStatusRepository {
    pool: PgPool,
}

impl PostgresStatusRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl StatusRepository for PostgresStatusRepository {
    async fn insert_group(&self, group: StatusGroup) -> CoreResult<()> {
        sqlx::query(
            r#"
            INSERT INTO status_groups (id, name, kind)
            VALUES ($1, $2, $3)
        "#,
        )
        .bind(group.id.0)
        .bind(group.name)
        .bind(status_group_kind_to_str(&group.kind))
        .execute(&self.pool)
        .await
        .map_err(map_storage_err)?;
        Ok(())
    }

    async fn insert_status(&self, status: Status) -> CoreResult<()> {
        sqlx::query(
            r#"
            INSERT INTO statuses (id, project_id, name, group_id, "order")
            VALUES ($1, $2, $3, $4, $5)
        "#,
        )
        .bind(status.id.0)
        .bind(status.project_id.map(|p| p.0))
        .bind(status.name)
        .bind(status.group_id.0)
        .bind(status.order)
        .execute(&self.pool)
        .await
        .map_err(map_storage_err)?;
        Ok(())
    }

    async fn list_groups(&self) -> CoreResult<Vec<StatusGroup>> {
        let rows = sqlx::query("SELECT * FROM status_groups ORDER BY name")
            .fetch_all(&self.pool)
            .await
            .map_err(map_storage_err)?;

        let mut groups = Vec::new();
        for row in rows {
            let kind_str: String = row.try_get("kind").map_err(map_storage_err)?;
            groups.push(StatusGroup {
                id: StatusGroupId::from(
                    row.try_get::<uuid::Uuid, _>("id")
                        .map_err(map_storage_err)?,
                ),
                name: row.try_get("name").map_err(map_storage_err)?,
                kind: status_kind_from_str(&kind_str),
            });
        }
        Ok(groups)
    }

    async fn list_statuses_for_project(
        &self,
        project_id: Option<ProjectId>,
    ) -> CoreResult<Vec<Status>> {
        let rows = match project_id {
            Some(pid) => {
                sqlx::query(r#"SELECT * FROM statuses WHERE project_id = $1 ORDER BY "order""#)
                    .bind(pid.0)
                    .fetch_all(&self.pool)
                    .await
                    .map_err(map_storage_err)?
            }
            None => {
                sqlx::query(r#"SELECT * FROM statuses WHERE project_id IS NULL ORDER BY "order""#)
                    .fetch_all(&self.pool)
                    .await
                    .map_err(map_storage_err)?
            }
        };

        let mut statuses = Vec::new();
        for row in rows {
            statuses.push(Status {
                id: StatusId::from(
                    row.try_get::<uuid::Uuid, _>("id")
                        .map_err(map_storage_err)?,
                ),
                project_id: row
                    .try_get::<Option<uuid::Uuid>, _>("project_id")
                    .map_err(map_storage_err)?
                    .map(ProjectId::from),
                name: row.try_get("name").map_err(map_storage_err)?,
                group_id: StatusGroupId::from(
                    row.try_get::<uuid::Uuid, _>("group_id")
                        .map_err(map_storage_err)?,
                ),
                order: row.try_get("order").map_err(map_storage_err)?,
            });
        }
        Ok(statuses)
    }
}

#[derive(Clone)]
pub struct PostgresScheduleBlockRepository {
    pool: PgPool,
}

impl PostgresScheduleBlockRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl ScheduleBlockRepository for PostgresScheduleBlockRepository {
    async fn insert(&self, block: ScheduleBlock) -> CoreResult<()> {
        insert_schedule_block(&self.pool, block).await
    }

    async fn find(&self, id: ScheduleBlockId) -> CoreResult<Option<ScheduleBlock>> {
        let row = sqlx::query("SELECT * FROM schedule_blocks WHERE id = $1")
            .bind(id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(map_storage_err)?;

        row.map(row_to_schedule_block)
            .transpose()
            .map_err(map_storage_err)
    }

    async fn list_for_day(&self, day: Date) -> CoreResult<Vec<ScheduleBlock>> {
        let (start, end) = day_bounds(day).map_err(map_storage_err)?;
        let rows = sqlx::query(
            r#"
            SELECT *
            FROM schedule_blocks
            WHERE start_at >= $1 AND start_at < $2
            ORDER BY start_at, title_snapshot
        "#,
        )
        .bind(start)
        .bind(end)
        .fetch_all(&self.pool)
        .await
        .map_err(map_storage_err)?;

        rows.into_iter()
            .map(row_to_schedule_block)
            .map(|r| r.map_err(map_storage_err))
            .collect()
    }

    async fn replace_proposed_for_day(
        &self,
        day: Date,
        blocks: Vec<ScheduleBlock>,
    ) -> CoreResult<()> {
        let (start, end) = day_bounds(day).map_err(map_storage_err)?;
        let mut tx = self.pool.begin().await.map_err(map_storage_err)?;

        sqlx::query(
            r#"
            DELETE FROM schedule_blocks
            WHERE source = 'SCHEDULER'
              AND state = 'PROPOSED'
              AND start_at >= $1
              AND start_at < $2
        "#,
        )
        .bind(start)
        .bind(end)
        .execute(&mut *tx)
        .await
        .map_err(map_storage_err)?;

        for block in blocks {
            insert_schedule_block_in_tx(&mut tx, block).await?;
        }

        tx.commit().await.map_err(map_storage_err)?;
        Ok(())
    }
}

async fn insert_schedule_block(pool: &PgPool, block: ScheduleBlock) -> CoreResult<()> {
    sqlx::query(
        r#"
        INSERT INTO schedule_blocks (
            id, task_id, title_snapshot, start_at, end_at, block_type,
            state, locked, source, required_minutes, created_at, updated_at
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
    "#,
    )
    .bind(block.id.0)
    .bind(block.task_id.map(|id| id.0))
    .bind(block.title_snapshot)
    .bind(block.start_at)
    .bind(block.end_at)
    .bind(schedule_block_type_to_str(&block.block_type))
    .bind(schedule_block_state_to_str(&block.state))
    .bind(block.locked)
    .bind(schedule_block_source_to_str(&block.source))
    .bind(block.required_minutes.map(i64::from))
    .bind(block.created_at)
    .bind(block.updated_at)
    .execute(pool)
    .await
    .map_err(map_storage_err)?;
    Ok(())
}

async fn insert_schedule_block_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    block: ScheduleBlock,
) -> CoreResult<()> {
    sqlx::query(
        r#"
        INSERT INTO schedule_blocks (
            id, task_id, title_snapshot, start_at, end_at, block_type,
            state, locked, source, required_minutes, created_at, updated_at
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
    "#,
    )
    .bind(block.id.0)
    .bind(block.task_id.map(|id| id.0))
    .bind(block.title_snapshot)
    .bind(block.start_at)
    .bind(block.end_at)
    .bind(schedule_block_type_to_str(&block.block_type))
    .bind(schedule_block_state_to_str(&block.state))
    .bind(block.locked)
    .bind(schedule_block_source_to_str(&block.source))
    .bind(block.required_minutes.map(i64::from))
    .bind(block.created_at)
    .bind(block.updated_at)
    .execute(&mut **tx)
    .await
    .map_err(map_storage_err)?;
    Ok(())
}

fn row_to_schedule_block(row: sqlx::postgres::PgRow) -> Result<ScheduleBlock> {
    let block_type: String = row.try_get("block_type")?;
    let state: String = row.try_get("state")?;
    let source: String = row.try_get("source")?;

    Ok(ScheduleBlock {
        id: ScheduleBlockId::from(row.try_get::<uuid::Uuid, _>("id")?),
        task_id: row
            .try_get::<Option<uuid::Uuid>, _>("task_id")?
            .map(TaskId::from),
        title_snapshot: row.try_get("title_snapshot")?,
        start_at: row.try_get("start_at")?,
        end_at: row.try_get("end_at")?,
        block_type: schedule_block_type_from_str(&block_type),
        state: schedule_block_state_from_str(&state),
        locked: row.try_get("locked")?,
        source: schedule_block_source_from_str(&source),
        required_minutes: row
            .try_get::<Option<i64>, _>("required_minutes")?
            .map(|value| value as u32),
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

#[derive(Clone)]
pub struct PostgresUserSettingsRepository {
    pool: PgPool,
}

impl PostgresUserSettingsRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl UserSettingsRepository for PostgresUserSettingsRepository {
    async fn upsert(&self, settings: UserSettings) -> CoreResult<()> {
        let automation = serde_json::to_value(&settings.automation).map_err(map_storage_err)?;
        let weekly = serde_json::to_value(&settings.weekly_review).map_err(map_storage_err)?;
        sqlx::query(
            r#"
            INSERT INTO user_settings (
                user_id, provider, model_for_planning, model_for_routine, automation, weekly_review
            ) VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT(user_id) DO UPDATE SET
                provider = excluded.provider,
                model_for_planning = excluded.model_for_planning,
                model_for_routine = excluded.model_for_routine,
                automation = excluded.automation,
                weekly_review = excluded.weekly_review
        "#,
        )
        .bind(settings.user_id.0)
        .bind(llm_provider_to_string(&settings.provider))
        .bind(settings.model_for_planning)
        .bind(settings.model_for_routine)
        .bind(automation)
        .bind(weekly)
        .execute(&self.pool)
        .await
        .map_err(map_storage_err)?;
        Ok(())
    }

    async fn get(&self, user_id: UserId) -> CoreResult<Option<UserSettings>> {
        let row = sqlx::query("SELECT * FROM user_settings WHERE user_id = $1")
            .bind(user_id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(map_storage_err)?;

        row.map(|row| -> Result<UserSettings> {
            let provider_str: String = row.try_get("provider")?;
            let automation_value: serde_json::Value = row.try_get("automation")?;
            let weekly_value: serde_json::Value = row.try_get("weekly_review")?;

            Ok(UserSettings {
                user_id,
                provider: llm_provider_from_string(&provider_str),
                model_for_planning: row.try_get("model_for_planning")?,
                model_for_routine: row.try_get("model_for_routine")?,
                automation: serde_json::from_value(automation_value)?,
                weekly_review: serde_json::from_value(weekly_value)?,
            })
        })
        .transpose()
        .map_err(map_storage_err)
    }
}

#[derive(Clone)]
pub struct SqliteTaskRepository {
    pool: SqlitePool,
}

impl SqliteTaskRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl TaskRepository for SqliteTaskRepository {
    async fn insert(&self, task: Task) -> CoreResult<()> {
        let dependencies = serde_json::to_string(&task.dependencies).map_err(map_storage_err)?;
        sqlx::query(
            r#"
            INSERT INTO tasks (
                id, title, description, project_id, list_id, status_id,
                due_date, start_date, estimated_minutes, cost_points, dependencies,
                milestone_id, created_at, updated_at, deleted_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
        )
        .bind(task.id.0.to_string())
        .bind(task.title)
        .bind(task.description)
        .bind(task.project_id.map(|p| p.0.to_string()))
        .bind(task.list_id.map(|l| l.0.to_string()))
        .bind(task.status_id.0.to_string())
        .bind(task.due_date.map(date_to_string))
        .bind(task.start_date.map(date_to_string))
        .bind(task.estimated_minutes.map(i64::from))
        .bind(task.cost_points.map(i64::from))
        .bind(dependencies)
        .bind(task.milestone_id.map(|m| m.0.to_string()))
        .bind(to_rfc3339(task.created_at).map_err(map_storage_err)?)
        .bind(to_rfc3339(task.updated_at).map_err(map_storage_err)?)
        .bind(
            task.deleted_at
                .map(|d| to_rfc3339(d).map_err(map_storage_err))
                .transpose()?,
        )
        .execute(&self.pool)
        .await
        .map_err(map_storage_err)?;
        Ok(())
    }

    async fn find(&self, id: TaskId) -> CoreResult<Option<Task>> {
        let row = sqlx::query("SELECT * FROM tasks WHERE id = ?")
            .bind(id.0.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(map_storage_err)?;

        row.map(row_to_task_sqlite)
            .transpose()
            .map_err(map_storage_err)
    }

    async fn update(&self, task: Task) -> CoreResult<()> {
        let dependencies = serde_json::to_string(&task.dependencies).map_err(map_storage_err)?;
        let rows = sqlx::query(
            r#"
            UPDATE tasks SET
                title = ?,
                description = ?,
                project_id = ?,
                list_id = ?,
                status_id = ?,
                due_date = ?,
                start_date = ?,
                estimated_minutes = ?,
                cost_points = ?,
                dependencies = ?,
                milestone_id = ?,
                created_at = ?,
                updated_at = ?,
                deleted_at = ?
            WHERE id = ?
        "#,
        )
        .bind(task.title)
        .bind(task.description)
        .bind(task.project_id.map(|p| p.0.to_string()))
        .bind(task.list_id.map(|l| l.0.to_string()))
        .bind(task.status_id.0.to_string())
        .bind(task.due_date.map(date_to_string))
        .bind(task.start_date.map(date_to_string))
        .bind(task.estimated_minutes.map(i64::from))
        .bind(task.cost_points.map(i64::from))
        .bind(dependencies)
        .bind(task.milestone_id.map(|m| m.0.to_string()))
        .bind(to_rfc3339(task.created_at).map_err(map_storage_err)?)
        .bind(to_rfc3339(task.updated_at).map_err(map_storage_err)?)
        .bind(
            task.deleted_at
                .map(|d| to_rfc3339(d).map_err(map_storage_err))
                .transpose()?,
        )
        .bind(task.id.0.to_string())
        .execute(&self.pool)
        .await
        .map_err(map_storage_err)?;
        if rows.rows_affected() == 0 {
            return Err(CoreError::NotFound);
        }
        Ok(())
    }

    async fn list_all(&self) -> CoreResult<Vec<Task>> {
        let rows = sqlx::query("SELECT * FROM tasks ORDER BY created_at, title")
            .fetch_all(&self.pool)
            .await
            .map_err(map_storage_err)?;

        rows.into_iter()
            .map(row_to_task_sqlite)
            .map(|r| r.map_err(map_storage_err))
            .collect()
    }

    async fn list_by_project(&self, project_id: ProjectId) -> CoreResult<Vec<Task>> {
        let rows = sqlx::query("SELECT * FROM tasks WHERE project_id = ?")
            .bind(project_id.0.to_string())
            .fetch_all(&self.pool)
            .await
            .map_err(map_storage_err)?;

        rows.into_iter()
            .map(row_to_task_sqlite)
            .map(|r| r.map_err(map_storage_err))
            .collect()
    }

    async fn list_by_list(&self, list_id: ListId) -> CoreResult<Vec<Task>> {
        let rows = sqlx::query("SELECT * FROM tasks WHERE list_id = ?")
            .bind(list_id.0.to_string())
            .fetch_all(&self.pool)
            .await
            .map_err(map_storage_err)?;

        rows.into_iter()
            .map(row_to_task_sqlite)
            .map(|r| r.map_err(map_storage_err))
            .collect()
    }

    async fn soft_delete(&self, id: TaskId, deleted_at: OffsetDateTime) -> CoreResult<()> {
        let res = sqlx::query("UPDATE tasks SET deleted_at = ? WHERE id = ?")
            .bind(to_rfc3339(deleted_at).map_err(map_storage_err)?)
            .bind(id.0.to_string())
            .execute(&self.pool)
            .await
            .map_err(map_storage_err)?;
        if res.rows_affected() == 0 {
            return Err(CoreError::NotFound);
        }
        Ok(())
    }
}

fn row_to_task_sqlite(row: sqlx::sqlite::SqliteRow) -> Result<Task> {
    let dependency_ids: Vec<uuid::Uuid> =
        serde_json::from_str(&row.try_get::<String, _>("dependencies")?)?;

    Ok(Task {
        id: TaskId::from(uuid::Uuid::parse_str(&row.try_get::<String, _>("id")?)?),
        title: row.try_get("title")?,
        description: row.try_get("description")?,
        project_id: row
            .try_get::<Option<String>, _>("project_id")?
            .map(|s| uuid::Uuid::parse_str(&s))
            .transpose()?
            .map(ProjectId::from),
        list_id: row
            .try_get::<Option<String>, _>("list_id")?
            .map(|s| uuid::Uuid::parse_str(&s))
            .transpose()?
            .map(ListId::from),
        status_id: StatusId::from(uuid::Uuid::parse_str(
            &row.try_get::<String, _>("status_id")?,
        )?),
        due_date: row
            .try_get::<Option<String>, _>("due_date")?
            .as_deref()
            .map(date_from_string)
            .transpose()?,
        start_date: row
            .try_get::<Option<String>, _>("start_date")?
            .as_deref()
            .map(date_from_string)
            .transpose()?,
        estimated_minutes: row
            .try_get::<Option<i64>, _>("estimated_minutes")?
            .map(|v| v as u32),
        cost_points: row
            .try_get::<Option<i64>, _>("cost_points")?
            .map(|v| v as u32),
        dependencies: dependency_ids.into_iter().map(TaskId::from).collect(),
        milestone_id: row
            .try_get::<Option<String>, _>("milestone_id")?
            .map(|s| uuid::Uuid::parse_str(&s))
            .transpose()?
            .map(MilestoneId::from),
        created_at: from_rfc3339(&row.try_get::<String, _>("created_at")?)?,
        updated_at: from_rfc3339(&row.try_get::<String, _>("updated_at")?)?,
        deleted_at: row
            .try_get::<Option<String>, _>("deleted_at")?
            .as_deref()
            .map(from_rfc3339)
            .transpose()?,
    })
}

#[derive(Clone)]
pub struct SqliteProjectRepository {
    pool: SqlitePool,
}

impl SqliteProjectRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl ProjectRepository for SqliteProjectRepository {
    async fn insert(&self, project: Project) -> CoreResult<()> {
        sqlx::query(
            r#"
            INSERT INTO projects (
                id, title, description, start_date, end_date, default_status_set_id, archived_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
        )
        .bind(project.id.0.to_string())
        .bind(project.title)
        .bind(project.description)
        .bind(project.start_date.map(date_to_string))
        .bind(project.end_date.map(date_to_string))
        .bind(project.default_status_set_id.map(|id| id.0.to_string()))
        .bind(
            project
                .archived_at
                .map(|d| to_rfc3339(d).map_err(map_storage_err))
                .transpose()?,
        )
        .execute(&self.pool)
        .await
        .map_err(map_storage_err)?;
        Ok(())
    }

    async fn find(&self, id: ProjectId) -> CoreResult<Option<Project>> {
        let row = sqlx::query("SELECT * FROM projects WHERE id = ?")
            .bind(id.0.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(map_storage_err)?;

        row.map(row_to_project_sqlite)
            .transpose()
            .map_err(map_storage_err)
    }

    async fn list_all(&self) -> CoreResult<Vec<Project>> {
        let rows = sqlx::query("SELECT * FROM projects ORDER BY title")
            .fetch_all(&self.pool)
            .await
            .map_err(map_storage_err)?;

        rows.into_iter()
            .map(row_to_project_sqlite)
            .map(|r| r.map_err(map_storage_err))
            .collect()
    }

    async fn update(&self, project: Project) -> CoreResult<()> {
        let rows = sqlx::query(
            r#"
            UPDATE projects SET
                title = ?,
                description = ?,
                start_date = ?,
                end_date = ?,
                default_status_set_id = ?,
                archived_at = ?
            WHERE id = ?
        "#,
        )
        .bind(project.title)
        .bind(project.description)
        .bind(project.start_date.map(date_to_string))
        .bind(project.end_date.map(date_to_string))
        .bind(project.default_status_set_id.map(|id| id.0.to_string()))
        .bind(
            project
                .archived_at
                .map(|d| to_rfc3339(d).map_err(map_storage_err))
                .transpose()?,
        )
        .bind(project.id.0.to_string())
        .execute(&self.pool)
        .await
        .map_err(map_storage_err)?;
        if rows.rows_affected() == 0 {
            return Err(CoreError::NotFound);
        }
        Ok(())
    }
}

fn row_to_project_sqlite(row: sqlx::sqlite::SqliteRow) -> Result<Project> {
    Ok(Project {
        id: ProjectId::from(uuid::Uuid::parse_str(&row.try_get::<String, _>("id")?)?),
        title: row.try_get("title")?,
        description: row.try_get("description")?,
        start_date: row
            .try_get::<Option<String>, _>("start_date")?
            .as_deref()
            .map(date_from_string)
            .transpose()?,
        end_date: row
            .try_get::<Option<String>, _>("end_date")?
            .as_deref()
            .map(date_from_string)
            .transpose()?,
        default_status_set_id: row
            .try_get::<Option<String>, _>("default_status_set_id")?
            .map(|s| uuid::Uuid::parse_str(&s))
            .transpose()?
            .map(StatusGroupId::from),
        archived_at: row
            .try_get::<Option<String>, _>("archived_at")?
            .as_deref()
            .map(from_rfc3339)
            .transpose()?,
    })
}

#[derive(Clone)]
pub struct SqliteListRepository {
    pool: SqlitePool,
}

impl SqliteListRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl ListRepository for SqliteListRepository {
    async fn insert(&self, list: List) -> CoreResult<()> {
        sqlx::query(
            r#"
            INSERT INTO lists (id, project_id, name, is_system, kind, view_type, "order")
            VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
        )
        .bind(list.id.0.to_string())
        .bind(list.project_id.map(|p| p.0.to_string()))
        .bind(list.name)
        .bind(if list.is_system { 1 } else { 0 })
        .bind(list_kind_to_str(&list.kind))
        .bind(list_view_to_str(&list.view_type))
        .bind(list.order)
        .execute(&self.pool)
        .await
        .map_err(map_storage_err)?;
        Ok(())
    }

    async fn find(&self, id: ListId) -> CoreResult<Option<List>> {
        let row = sqlx::query("SELECT * FROM lists WHERE id = ?")
            .bind(id.0.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(map_storage_err)?;

        row.map(row_to_list_sqlite)
            .transpose()
            .map_err(map_storage_err)
    }

    async fn list_by_project(&self, project_id: ProjectId) -> CoreResult<Vec<List>> {
        let rows = sqlx::query(r#"SELECT * FROM lists WHERE project_id = ? ORDER BY "order""#)
            .bind(project_id.0.to_string())
            .fetch_all(&self.pool)
            .await
            .map_err(map_storage_err)?;

        rows.into_iter()
            .map(row_to_list_sqlite)
            .map(|r| r.map_err(map_storage_err))
            .collect()
    }

    async fn list_system(&self) -> CoreResult<Vec<List>> {
        let rows = sqlx::query(r#"SELECT * FROM lists WHERE is_system = 1 ORDER BY "order""#)
            .fetch_all(&self.pool)
            .await
            .map_err(map_storage_err)?;

        rows.into_iter()
            .map(row_to_list_sqlite)
            .map(|r| r.map_err(map_storage_err))
            .collect()
    }

    async fn update(&self, list: List) -> CoreResult<()> {
        let rows = sqlx::query(
            r#"
            UPDATE lists SET
                project_id = ?,
                name = ?,
                is_system = ?,
                kind = ?,
                view_type = ?,
                "order" = ?
            WHERE id = ?
        "#,
        )
        .bind(list.project_id.map(|p| p.0.to_string()))
        .bind(list.name)
        .bind(if list.is_system { 1 } else { 0 })
        .bind(list_kind_to_str(&list.kind))
        .bind(list_view_to_str(&list.view_type))
        .bind(list.order)
        .bind(list.id.0.to_string())
        .execute(&self.pool)
        .await
        .map_err(map_storage_err)?;
        if rows.rows_affected() == 0 {
            return Err(CoreError::NotFound);
        }
        Ok(())
    }
}

fn row_to_list_sqlite(row: sqlx::sqlite::SqliteRow) -> Result<List> {
    let kind: String = row.try_get("kind")?;
    let view_type: String = row.try_get("view_type")?;

    Ok(List {
        id: ListId::from(uuid::Uuid::parse_str(&row.try_get::<String, _>("id")?)?),
        project_id: row
            .try_get::<Option<String>, _>("project_id")?
            .map(|s| uuid::Uuid::parse_str(&s))
            .transpose()?
            .map(ProjectId::from),
        name: row.try_get("name")?,
        is_system: row.try_get::<i64, _>("is_system")? != 0,
        kind: list_kind_from_str(&kind),
        view_type: list_view_from_str(&view_type),
        order: row.try_get::<i64, _>("order")? as i32,
    })
}

#[derive(Clone)]
pub struct SqliteMilestoneRepository {
    pool: SqlitePool,
}

impl SqliteMilestoneRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl MilestoneRepository for SqliteMilestoneRepository {
    async fn insert(&self, milestone: Milestone) -> CoreResult<()> {
        let dependencies =
            serde_json::to_string(&milestone.dependency_task_ids).map_err(map_storage_err)?;
        sqlx::query(
            r#"
            INSERT INTO milestones (
                id, project_id, title, description, target_date, status,
                dependency_task_ids, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
        )
        .bind(milestone.id.0.to_string())
        .bind(milestone.project_id.0.to_string())
        .bind(milestone.title)
        .bind(milestone.description)
        .bind(date_to_string(milestone.target_date))
        .bind(milestone_status_to_str(&milestone.status))
        .bind(dependencies)
        .bind(to_rfc3339(milestone.created_at).map_err(map_storage_err)?)
        .bind(to_rfc3339(milestone.updated_at).map_err(map_storage_err)?)
        .execute(&self.pool)
        .await
        .map_err(map_storage_err)?;
        Ok(())
    }

    async fn find(&self, id: MilestoneId) -> CoreResult<Option<Milestone>> {
        let row = sqlx::query("SELECT * FROM milestones WHERE id = ?")
            .bind(id.0.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(map_storage_err)?;

        row.map(row_to_milestone_sqlite)
            .transpose()
            .map_err(map_storage_err)
    }

    async fn list_by_project(&self, project_id: ProjectId) -> CoreResult<Vec<Milestone>> {
        let rows =
            sqlx::query("SELECT * FROM milestones WHERE project_id = ? ORDER BY target_date")
                .bind(project_id.0.to_string())
                .fetch_all(&self.pool)
                .await
                .map_err(map_storage_err)?;

        rows.into_iter()
            .map(row_to_milestone_sqlite)
            .map(|r| r.map_err(map_storage_err))
            .collect()
    }

    async fn update(&self, milestone: Milestone) -> CoreResult<()> {
        let dependencies =
            serde_json::to_string(&milestone.dependency_task_ids).map_err(map_storage_err)?;
        let rows = sqlx::query(
            r#"
            UPDATE milestones SET
                project_id = ?,
                title = ?,
                description = ?,
                target_date = ?,
                status = ?,
                dependency_task_ids = ?,
                created_at = ?,
                updated_at = ?
            WHERE id = ?
        "#,
        )
        .bind(milestone.project_id.0.to_string())
        .bind(milestone.title)
        .bind(milestone.description)
        .bind(date_to_string(milestone.target_date))
        .bind(milestone_status_to_str(&milestone.status))
        .bind(dependencies)
        .bind(to_rfc3339(milestone.created_at).map_err(map_storage_err)?)
        .bind(to_rfc3339(milestone.updated_at).map_err(map_storage_err)?)
        .bind(milestone.id.0.to_string())
        .execute(&self.pool)
        .await
        .map_err(map_storage_err)?;
        if rows.rows_affected() == 0 {
            return Err(CoreError::NotFound);
        }
        Ok(())
    }
}

fn row_to_milestone_sqlite(row: sqlx::sqlite::SqliteRow) -> Result<Milestone> {
    let dependency_ids: Vec<uuid::Uuid> =
        serde_json::from_str(&row.try_get::<String, _>("dependency_task_ids")?)?;
    let status: String = row.try_get("status")?;

    Ok(Milestone {
        id: MilestoneId::from(uuid::Uuid::parse_str(&row.try_get::<String, _>("id")?)?),
        project_id: ProjectId::from(uuid::Uuid::parse_str(
            &row.try_get::<String, _>("project_id")?,
        )?),
        title: row.try_get("title")?,
        description: row.try_get("description")?,
        target_date: date_from_string(&row.try_get::<String, _>("target_date")?)?,
        status: milestone_status_from_str(&status),
        dependency_task_ids: dependency_ids.into_iter().map(TaskId::from).collect(),
        created_at: from_rfc3339(&row.try_get::<String, _>("created_at")?)?,
        updated_at: from_rfc3339(&row.try_get::<String, _>("updated_at")?)?,
    })
}

#[derive(Clone)]
pub struct SqliteStatusRepository {
    pool: SqlitePool,
}

impl SqliteStatusRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl StatusRepository for SqliteStatusRepository {
    async fn insert_group(&self, group: StatusGroup) -> CoreResult<()> {
        sqlx::query("INSERT INTO status_groups (id, name, kind) VALUES (?, ?, ?)")
            .bind(group.id.0.to_string())
            .bind(group.name)
            .bind(status_group_kind_to_str(&group.kind))
            .execute(&self.pool)
            .await
            .map_err(map_storage_err)?;
        Ok(())
    }

    async fn insert_status(&self, status: Status) -> CoreResult<()> {
        sqlx::query(
            r#"INSERT INTO statuses (id, project_id, name, group_id, "order") VALUES (?, ?, ?, ?, ?)"#,
        )
        .bind(status.id.0.to_string())
        .bind(status.project_id.map(|p| p.0.to_string()))
        .bind(status.name)
        .bind(status.group_id.0.to_string())
        .bind(status.order)
        .execute(&self.pool)
        .await
        .map_err(map_storage_err)?;
        Ok(())
    }

    async fn list_groups(&self) -> CoreResult<Vec<StatusGroup>> {
        let rows = sqlx::query("SELECT * FROM status_groups ORDER BY name")
            .fetch_all(&self.pool)
            .await
            .map_err(map_storage_err)?;

        rows.into_iter()
            .map(row_to_status_group_sqlite)
            .map(|r| r.map_err(map_storage_err))
            .collect()
    }

    async fn list_statuses_for_project(
        &self,
        project_id: Option<ProjectId>,
    ) -> CoreResult<Vec<Status>> {
        let rows = match project_id {
            Some(project_id) => {
                sqlx::query(r#"SELECT * FROM statuses WHERE project_id = ? ORDER BY "order""#)
                    .bind(project_id.0.to_string())
                    .fetch_all(&self.pool)
                    .await
                    .map_err(map_storage_err)?
            }
            None => {
                sqlx::query(r#"SELECT * FROM statuses WHERE project_id IS NULL ORDER BY "order""#)
                    .fetch_all(&self.pool)
                    .await
                    .map_err(map_storage_err)?
            }
        };

        rows.into_iter()
            .map(row_to_status_sqlite)
            .map(|r| r.map_err(map_storage_err))
            .collect()
    }
}

fn row_to_status_group_sqlite(row: sqlx::sqlite::SqliteRow) -> Result<StatusGroup> {
    let kind: String = row.try_get("kind")?;
    Ok(StatusGroup {
        id: StatusGroupId::from(uuid::Uuid::parse_str(&row.try_get::<String, _>("id")?)?),
        name: row.try_get("name")?,
        kind: status_kind_from_str(&kind),
    })
}

fn row_to_status_sqlite(row: sqlx::sqlite::SqliteRow) -> Result<Status> {
    Ok(Status {
        id: StatusId::from(uuid::Uuid::parse_str(&row.try_get::<String, _>("id")?)?),
        project_id: row
            .try_get::<Option<String>, _>("project_id")?
            .map(|s| uuid::Uuid::parse_str(&s))
            .transpose()?
            .map(ProjectId::from),
        name: row.try_get("name")?,
        group_id: StatusGroupId::from(uuid::Uuid::parse_str(
            &row.try_get::<String, _>("group_id")?,
        )?),
        order: row.try_get::<i64, _>("order")? as i32,
    })
}

#[derive(Clone)]
pub struct SqliteScheduleBlockRepository {
    pool: SqlitePool,
}

impl SqliteScheduleBlockRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl ScheduleBlockRepository for SqliteScheduleBlockRepository {
    async fn insert(&self, block: ScheduleBlock) -> CoreResult<()> {
        insert_schedule_block_sqlite(&self.pool, block).await
    }

    async fn find(&self, id: ScheduleBlockId) -> CoreResult<Option<ScheduleBlock>> {
        let row = sqlx::query("SELECT * FROM schedule_blocks WHERE id = ?")
            .bind(id.0.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(map_storage_err)?;

        row.map(row_to_schedule_block_sqlite)
            .transpose()
            .map_err(map_storage_err)
    }

    async fn list_for_day(&self, day: Date) -> CoreResult<Vec<ScheduleBlock>> {
        let (start, end) = day_bounds(day).map_err(map_storage_err)?;
        let rows = sqlx::query(
            r#"
            SELECT *
            FROM schedule_blocks
            WHERE start_at >= ? AND start_at < ?
            ORDER BY start_at, title_snapshot
        "#,
        )
        .bind(to_rfc3339(start).map_err(map_storage_err)?)
        .bind(to_rfc3339(end).map_err(map_storage_err)?)
        .fetch_all(&self.pool)
        .await
        .map_err(map_storage_err)?;

        rows.into_iter()
            .map(row_to_schedule_block_sqlite)
            .map(|r| r.map_err(map_storage_err))
            .collect()
    }

    async fn replace_proposed_for_day(
        &self,
        day: Date,
        blocks: Vec<ScheduleBlock>,
    ) -> CoreResult<()> {
        let (start, end) = day_bounds(day).map_err(map_storage_err)?;
        let mut tx = self.pool.begin().await.map_err(map_storage_err)?;

        sqlx::query(
            r#"
            DELETE FROM schedule_blocks
            WHERE source = 'SCHEDULER'
              AND state = 'PROPOSED'
              AND start_at >= ?
              AND start_at < ?
        "#,
        )
        .bind(to_rfc3339(start).map_err(map_storage_err)?)
        .bind(to_rfc3339(end).map_err(map_storage_err)?)
        .execute(&mut *tx)
        .await
        .map_err(map_storage_err)?;

        for block in blocks {
            insert_schedule_block_sqlite_in_tx(&mut tx, block).await?;
        }

        tx.commit().await.map_err(map_storage_err)?;
        Ok(())
    }
}

async fn insert_schedule_block_sqlite(pool: &SqlitePool, block: ScheduleBlock) -> CoreResult<()> {
    sqlx::query(
        r#"
        INSERT INTO schedule_blocks (
            id, task_id, title_snapshot, start_at, end_at, block_type,
            state, locked, source, required_minutes, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    "#,
    )
    .bind(block.id.0.to_string())
    .bind(block.task_id.map(|id| id.0.to_string()))
    .bind(block.title_snapshot)
    .bind(to_rfc3339(block.start_at).map_err(map_storage_err)?)
    .bind(to_rfc3339(block.end_at).map_err(map_storage_err)?)
    .bind(schedule_block_type_to_str(&block.block_type))
    .bind(schedule_block_state_to_str(&block.state))
    .bind(if block.locked { 1 } else { 0 })
    .bind(schedule_block_source_to_str(&block.source))
    .bind(block.required_minutes.map(i64::from))
    .bind(to_rfc3339(block.created_at).map_err(map_storage_err)?)
    .bind(to_rfc3339(block.updated_at).map_err(map_storage_err)?)
    .execute(pool)
    .await
    .map_err(map_storage_err)?;
    Ok(())
}

async fn insert_schedule_block_sqlite_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    block: ScheduleBlock,
) -> CoreResult<()> {
    sqlx::query(
        r#"
        INSERT INTO schedule_blocks (
            id, task_id, title_snapshot, start_at, end_at, block_type,
            state, locked, source, required_minutes, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    "#,
    )
    .bind(block.id.0.to_string())
    .bind(block.task_id.map(|id| id.0.to_string()))
    .bind(block.title_snapshot)
    .bind(to_rfc3339(block.start_at).map_err(map_storage_err)?)
    .bind(to_rfc3339(block.end_at).map_err(map_storage_err)?)
    .bind(schedule_block_type_to_str(&block.block_type))
    .bind(schedule_block_state_to_str(&block.state))
    .bind(if block.locked { 1 } else { 0 })
    .bind(schedule_block_source_to_str(&block.source))
    .bind(block.required_minutes.map(i64::from))
    .bind(to_rfc3339(block.created_at).map_err(map_storage_err)?)
    .bind(to_rfc3339(block.updated_at).map_err(map_storage_err)?)
    .execute(&mut **tx)
    .await
    .map_err(map_storage_err)?;
    Ok(())
}

fn row_to_schedule_block_sqlite(row: sqlx::sqlite::SqliteRow) -> Result<ScheduleBlock> {
    let block_type: String = row.try_get("block_type")?;
    let state: String = row.try_get("state")?;
    let source: String = row.try_get("source")?;

    Ok(ScheduleBlock {
        id: ScheduleBlockId::from(uuid::Uuid::parse_str(&row.try_get::<String, _>("id")?)?),
        task_id: row
            .try_get::<Option<String>, _>("task_id")?
            .map(|s| uuid::Uuid::parse_str(&s))
            .transpose()?
            .map(TaskId::from),
        title_snapshot: row.try_get("title_snapshot")?,
        start_at: from_rfc3339(&row.try_get::<String, _>("start_at")?)?,
        end_at: from_rfc3339(&row.try_get::<String, _>("end_at")?)?,
        block_type: schedule_block_type_from_str(&block_type),
        state: schedule_block_state_from_str(&state),
        locked: row.try_get::<i64, _>("locked")? != 0,
        source: schedule_block_source_from_str(&source),
        required_minutes: row
            .try_get::<Option<i64>, _>("required_minutes")?
            .map(|value| value as u32),
        created_at: from_rfc3339(&row.try_get::<String, _>("created_at")?)?,
        updated_at: from_rfc3339(&row.try_get::<String, _>("updated_at")?)?,
    })
}

#[derive(Clone)]
pub struct SqliteUserSettingsRepository {
    pool: SqlitePool,
}

impl SqliteUserSettingsRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl UserSettingsRepository for SqliteUserSettingsRepository {
    async fn upsert(&self, settings: UserSettings) -> CoreResult<()> {
        let automation = serde_json::to_string(&settings.automation).map_err(map_storage_err)?;
        let weekly_review =
            serde_json::to_string(&settings.weekly_review).map_err(map_storage_err)?;
        sqlx::query(
            r#"
            INSERT INTO user_settings (
                user_id, provider, model_for_planning, model_for_routine, automation, weekly_review
            ) VALUES (?, ?, ?, ?, ?, ?)
            ON CONFLICT(user_id) DO UPDATE SET
                provider = excluded.provider,
                model_for_planning = excluded.model_for_planning,
                model_for_routine = excluded.model_for_routine,
                automation = excluded.automation,
                weekly_review = excluded.weekly_review
        "#,
        )
        .bind(settings.user_id.0.to_string())
        .bind(llm_provider_to_string(&settings.provider))
        .bind(settings.model_for_planning)
        .bind(settings.model_for_routine)
        .bind(automation)
        .bind(weekly_review)
        .execute(&self.pool)
        .await
        .map_err(map_storage_err)?;
        Ok(())
    }

    async fn get(&self, user_id: UserId) -> CoreResult<Option<UserSettings>> {
        let row = sqlx::query("SELECT * FROM user_settings WHERE user_id = ?")
            .bind(user_id.0.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(map_storage_err)?;

        row.map(|row| -> Result<UserSettings> {
            let provider: String = row.try_get("provider")?;
            let automation: AutomationSettings =
                serde_json::from_str(&row.try_get::<String, _>("automation")?)?;
            let weekly_review: WeeklyReviewSettings =
                serde_json::from_str(&row.try_get::<String, _>("weekly_review")?)?;

            Ok(UserSettings {
                user_id,
                provider: llm_provider_from_string(&provider),
                model_for_planning: row.try_get("model_for_planning")?,
                model_for_routine: row.try_get("model_for_routine")?,
                automation,
                weekly_review,
            })
        })
        .transpose()
        .map_err(map_storage_err)
    }
}
