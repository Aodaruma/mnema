use anyhow::Result;
use mnema_core::prelude::*;
use sqlx::{Row, SqlitePool};
use time::format_description::well_known::Rfc3339;
use time::{Date, OffsetDateTime};

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
    dt.format(&Rfc3339).map_err(|e| anyhow::anyhow!(e))
}

fn from_rfc3339(s: &str) -> Result<OffsetDateTime> {
    OffsetDateTime::parse(s, &Rfc3339).map_err(|e| anyhow::anyhow!(e))
}

fn date_to_string(date: Date) -> Result<String> {
    Ok(date.to_string())
}

fn date_from_string(s: &str) -> Result<Date> {
    Date::parse(s, time::macros::format_description!("[year]-[month]-[day]"))
        .map_err(|e| anyhow::anyhow!(e))
}

fn map_storage_err(e: impl ToString) -> CoreError {
    CoreError::Storage(e.to_string())
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
        let dependencies = serde_json::to_string(&task.dependencies)
            .map_err(|e| CoreError::Storage(e.to_string()))?;
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
        .bind(
            task.due_date
                .map(|d| date_to_string(d).map_err(map_storage_err))
                .transpose()?,
        )
        .bind(
            task.start_date
                .map(|d| date_to_string(d).map_err(map_storage_err))
                .transpose()?,
        )
        .bind(task.estimated_minutes.map(|v| v as i64))
        .bind(task.cost_points.map(|v| v as i64))
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
        let row = sqlx::query(
            r#"
            SELECT *
            FROM tasks
            WHERE id = ?
        "#,
        )
        .bind(id.0.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_storage_err)?;

        match row {
            None => Ok(None),
            Some(row) => Ok(Some(row_to_task(row).map_err(map_storage_err)?)),
        }
    }

    async fn update(&self, task: Task) -> CoreResult<()> {
        let dependencies = serde_json::to_string(&task.dependencies)
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        let rows = sqlx::query(
            r#"
            UPDATE tasks SET
                title = ?, description = ?, project_id = ?, list_id = ?, status_id = ?,
                due_date = ?, start_date = ?, estimated_minutes = ?, cost_points = ?, dependencies = ?,
                milestone_id = ?, created_at = ?, updated_at = ?, deleted_at = ?
            WHERE id = ?
        "#,
        )
        .bind(task.title)
        .bind(task.description)
        .bind(task.project_id.map(|p| p.0.to_string()))
        .bind(task.list_id.map(|l| l.0.to_string()))
        .bind(task.status_id.0.to_string())
        .bind(task.due_date.map(|d| date_to_string(d).map_err(map_storage_err)).transpose()?)
        .bind(task.start_date.map(|d| date_to_string(d).map_err(map_storage_err)).transpose()?)
        .bind(task.estimated_minutes.map(|v| v as i64))
        .bind(task.cost_points.map(|v| v as i64))
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

    async fn list_by_project(&self, project_id: ProjectId) -> CoreResult<Vec<Task>> {
        let rows = sqlx::query(
            r#"
            SELECT * FROM tasks WHERE project_id = ?
        "#,
        )
        .bind(project_id.0.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_storage_err)?;

        rows.into_iter()
            .map(|row| row_to_task(row).map_err(map_storage_err))
            .collect()
    }

    async fn list_by_list(&self, list_id: ListId) -> CoreResult<Vec<Task>> {
        let rows = sqlx::query(
            r#"
            SELECT * FROM tasks WHERE list_id = ?
        "#,
        )
        .bind(list_id.0.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_storage_err)?;

        rows.into_iter()
            .map(|row| row_to_task(row).map_err(map_storage_err))
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

fn row_to_task(row: sqlx::sqlite::SqliteRow) -> Result<Task> {
    let dependency_ids: Vec<uuid::Uuid> =
        serde_json::from_str(row.try_get::<String, _>("dependencies")?.as_str())?;
    let dependencies: Vec<TaskId> = dependency_ids.into_iter().map(TaskId::from).collect();

    Ok(Task {
        id: TaskId::from(uuid::Uuid::parse_str(
            row.try_get::<String, _>("id")?.as_str(),
        )?),
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
            row.try_get::<String, _>("status_id")?.as_str(),
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
        dependencies,
        milestone_id: row
            .try_get::<Option<String>, _>("milestone_id")?
            .map(|s| uuid::Uuid::parse_str(&s))
            .transpose()?
            .map(MilestoneId::from),
        created_at: from_rfc3339(row.try_get::<String, _>("created_at")?.as_str())?,
        updated_at: from_rfc3339(row.try_get::<String, _>("updated_at")?.as_str())?,
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
            INSERT INTO projects (id, title, description, start_date, end_date, default_status_set_id, archived_at)
            VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
        )
        .bind(project.id.0.to_string())
        .bind(project.title)
        .bind(project.description)
        .bind(project.start_date.map(|d| date_to_string(d).map_err(map_storage_err)).transpose()?)
        .bind(project.end_date.map(|d| date_to_string(d).map_err(map_storage_err)).transpose()?)
        .bind(
            project
                .default_status_set_id
                .map(|id| id.0.to_string()),
        )
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

        let project = row
            .map(|row| -> Result<Project> {
                Ok(Project {
                    id: ProjectId::from(uuid::Uuid::parse_str(
                        row.try_get::<String, _>("id")?.as_str(),
                    )?),
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
            })
            .transpose()
            .map_err(map_storage_err)?;

        Ok(project)
    }

    async fn list_all(&self) -> CoreResult<Vec<Project>> {
        let rows = sqlx::query("SELECT * FROM projects")
            .fetch_all(&self.pool)
            .await
            .map_err(map_storage_err)?;

        let mut projects = Vec::new();
        for row in rows {
            let id = uuid::Uuid::parse_str(
                row.try_get::<String, _>("id")
                    .map_err(map_storage_err)?
                    .as_str(),
            )
            .map_err(map_storage_err)?;
            let title = row.try_get("title").map_err(map_storage_err)?;
            let description = row.try_get("description").map_err(map_storage_err)?;
            let start_date = row
                .try_get::<Option<String>, _>("start_date")
                .map_err(map_storage_err)?
                .as_deref()
                .map(date_from_string)
                .transpose()
                .map_err(map_storage_err)?;
            let end_date = row
                .try_get::<Option<String>, _>("end_date")
                .map_err(map_storage_err)?
                .as_deref()
                .map(date_from_string)
                .transpose()
                .map_err(map_storage_err)?;
            let default_status_set_id = row
                .try_get::<Option<String>, _>("default_status_set_id")
                .map_err(map_storage_err)?
                .map(|s| uuid::Uuid::parse_str(&s).map_err(map_storage_err))
                .transpose()?
                .map(StatusGroupId::from);
            let archived_at = row
                .try_get::<Option<String>, _>("archived_at")
                .map_err(map_storage_err)?
                .as_deref()
                .map(from_rfc3339)
                .transpose()
                .map_err(map_storage_err)?;

            projects.push(Project {
                id: ProjectId::from(id),
                title,
                description,
                start_date,
                end_date,
                default_status_set_id,
                archived_at,
            });
        }
        Ok(projects)
    }

    async fn update(&self, project: Project) -> CoreResult<()> {
        sqlx::query(
            r#"
            UPDATE projects SET
                title = ?, description = ?, start_date = ?, end_date = ?,
                default_status_set_id = ?, archived_at = ?
            WHERE id = ?
        "#,
        )
        .bind(project.title)
        .bind(project.description)
        .bind(
            project
                .start_date
                .map(|d| date_to_string(d).map_err(map_storage_err))
                .transpose()?,
        )
        .bind(
            project
                .end_date
                .map(|d| date_to_string(d).map_err(map_storage_err))
                .transpose()?,
        )
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
        Ok(())
    }
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

        Ok(row.map(row_to_list).transpose().map_err(map_storage_err)?)
    }

    async fn list_by_project(&self, project_id: ProjectId) -> CoreResult<Vec<List>> {
        let rows = sqlx::query("SELECT * FROM lists WHERE project_id = ?")
            .bind(project_id.0.to_string())
            .fetch_all(&self.pool)
            .await
            .map_err(map_storage_err)?;
        rows.into_iter()
            .map(row_to_list)
            .map(|r| r.map_err(map_storage_err))
            .collect()
    }

    async fn list_system(&self) -> CoreResult<Vec<List>> {
        let rows = sqlx::query("SELECT * FROM lists WHERE is_system = 1")
            .fetch_all(&self.pool)
            .await
            .map_err(map_storage_err)?;
        rows.into_iter()
            .map(row_to_list)
            .map(|r| r.map_err(map_storage_err))
            .collect()
    }

    async fn update(&self, list: List) -> CoreResult<()> {
        sqlx::query(
            r#"
            UPDATE lists SET
                project_id = ?, name = ?, is_system = ?, kind = ?, view_type = ?, "order" = ?
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
        Ok(())
    }
}

fn row_to_list(row: sqlx::sqlite::SqliteRow) -> Result<List> {
    let kind_str: String = row.try_get("kind")?;
    let view_str: String = row.try_get("view_type")?;
    let kind = list_kind_from_str(&kind_str);
    let view_type = list_view_from_str(&view_str);

    Ok(List {
        id: ListId::from(uuid::Uuid::parse_str(
            row.try_get::<String, _>("id")?.as_str(),
        )?),
        project_id: row
            .try_get::<Option<String>, _>("project_id")?
            .map(|s| uuid::Uuid::parse_str(&s))
            .transpose()?
            .map(ProjectId::from),
        name: row.try_get("name")?,
        is_system: row.try_get::<i64, _>("is_system")? != 0,
        kind,
        view_type,
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
        let deps = serde_json::to_string(&milestone.dependency_task_ids)
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        sqlx::query(
            r#"
            INSERT INTO milestones (
                id, project_id, title, description, target_date, status,
                dependency_task_ids, created_at, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
        )
        .bind(milestone.id.0.to_string())
        .bind(milestone.project_id.0.to_string())
        .bind(milestone.title)
        .bind(milestone.description)
        .bind(date_to_string(milestone.target_date).map_err(map_storage_err)?)
        .bind(milestone_status_to_str(&milestone.status))
        .bind(deps)
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

        Ok(row
            .map(row_to_milestone)
            .transpose()
            .map_err(map_storage_err)?)
    }

    async fn list_by_project(&self, project_id: ProjectId) -> CoreResult<Vec<Milestone>> {
        let rows = sqlx::query("SELECT * FROM milestones WHERE project_id = ?")
            .bind(project_id.0.to_string())
            .fetch_all(&self.pool)
            .await
            .map_err(map_storage_err)?;
        rows.into_iter()
            .map(row_to_milestone)
            .map(|r| r.map_err(map_storage_err))
            .collect()
    }

    async fn update(&self, milestone: Milestone) -> CoreResult<()> {
        let deps = serde_json::to_string(&milestone.dependency_task_ids)
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        sqlx::query(
            r#"
            UPDATE milestones SET
                project_id = ?, title = ?, description = ?, target_date = ?, status = ?,
                dependency_task_ids = ?, created_at = ?, updated_at = ?
            WHERE id = ?
        "#,
        )
        .bind(milestone.project_id.0.to_string())
        .bind(milestone.title)
        .bind(milestone.description)
        .bind(date_to_string(milestone.target_date).map_err(map_storage_err)?)
        .bind(milestone_status_to_str(&milestone.status))
        .bind(deps)
        .bind(to_rfc3339(milestone.created_at).map_err(map_storage_err)?)
        .bind(to_rfc3339(milestone.updated_at).map_err(map_storage_err)?)
        .bind(milestone.id.0.to_string())
        .execute(&self.pool)
        .await
        .map_err(map_storage_err)?;
        Ok(())
    }
}

fn row_to_milestone(row: sqlx::sqlite::SqliteRow) -> Result<Milestone> {
    let dep_ids: Vec<uuid::Uuid> =
        serde_json::from_str(row.try_get::<String, _>("dependency_task_ids")?.as_str())?;
    let deps: Vec<TaskId> = dep_ids.into_iter().map(TaskId::from).collect();
    let status_str: String = row.try_get("status")?;
    let status = milestone_status_from_str(&status_str);
    Ok(Milestone {
        id: MilestoneId::from(uuid::Uuid::parse_str(
            row.try_get::<String, _>("id")?.as_str(),
        )?),
        project_id: ProjectId::from(uuid::Uuid::parse_str(
            row.try_get::<String, _>("project_id")?.as_str(),
        )?),
        title: row.try_get("title")?,
        description: row.try_get("description")?,
        target_date: date_from_string(row.try_get::<String, _>("target_date")?.as_str())?,
        status,
        dependency_task_ids: deps,
        created_at: from_rfc3339(row.try_get::<String, _>("created_at")?.as_str())?,
        updated_at: from_rfc3339(row.try_get::<String, _>("updated_at")?.as_str())?,
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
        sqlx::query(
            r#"
            INSERT INTO status_groups (id, name, kind)
            VALUES (?, ?, ?)
        "#,
        )
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
            r#"
            INSERT INTO statuses (id, project_id, name, group_id, "order")
            VALUES (?, ?, ?, ?, ?)
        "#,
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
        let rows = sqlx::query("SELECT * FROM status_groups")
            .fetch_all(&self.pool)
            .await
            .map_err(map_storage_err)?;

        let mut groups = Vec::new();
        for row in rows {
            let kind_str: String = row.try_get("kind").map_err(map_storage_err)?;
            let kind = status_kind_from_str(&kind_str);
            let id = uuid::Uuid::parse_str(
                row.try_get::<String, _>("id")
                    .map_err(map_storage_err)?
                    .as_str(),
            )
            .map_err(map_storage_err)?;
            let name = row.try_get("name").map_err(map_storage_err)?;
            groups.push(StatusGroup {
                id: StatusGroupId::from(id),
                name,
                kind,
            });
        }
        Ok(groups)
    }

    async fn list_statuses_for_project(
        &self,
        project_id: Option<ProjectId>,
    ) -> CoreResult<Vec<Status>> {
        let rows = match project_id {
            Some(pid) => sqlx::query("SELECT * FROM statuses WHERE project_id = ?")
                .bind(pid.0.to_string())
                .fetch_all(&self.pool)
                .await
                .map_err(map_storage_err)?,
            None => sqlx::query("SELECT * FROM statuses WHERE project_id IS NULL")
                .fetch_all(&self.pool)
                .await
                .map_err(map_storage_err)?,
        };

        let mut statuses = Vec::new();
        for row in rows {
            let id = uuid::Uuid::parse_str(
                row.try_get::<String, _>("id")
                    .map_err(map_storage_err)?
                    .as_str(),
            )
            .map_err(map_storage_err)?;
            let project_id = row
                .try_get::<Option<String>, _>("project_id")
                .map_err(map_storage_err)?
                .map(|s| uuid::Uuid::parse_str(&s).map_err(map_storage_err))
                .transpose()?
                .map(ProjectId::from);
            let name = row.try_get("name").map_err(map_storage_err)?;
            let group_id = uuid::Uuid::parse_str(
                row.try_get::<String, _>("group_id")
                    .map_err(map_storage_err)?
                    .as_str(),
            )
            .map_err(map_storage_err)?;
            let order = row.try_get::<i64, _>("order").map_err(map_storage_err)? as i32;

            statuses.push(Status {
                id: StatusId::from(id),
                project_id,
                name,
                group_id: StatusGroupId::from(group_id),
                order,
            });
        }
        Ok(statuses)
    }
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
        let automation = serde_json::to_string(&settings.automation)
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        let weekly = serde_json::to_string(&settings.weekly_review)
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        sqlx::query(
            r#"
            INSERT INTO user_settings (user_id, provider, model_for_planning, model_for_routine, automation, weekly_review)
            VALUES (?, ?, ?, ?, ?, ?)
            ON CONFLICT(user_id) DO UPDATE SET
                provider = excluded.provider,
                model_for_planning = excluded.model_for_planning,
                model_for_routine = excluded.model_for_routine,
                automation = excluded.automation,
                weekly_review = excluded.weekly_review
        "#,
        )
        .bind(settings.user_id.0.to_string())
        .bind(match settings.provider {
            LlmProvider::Local => "LOCAL".to_string(),
            LlmProvider::OpenAiCompatible => "OPEN_AI_COMPATIBLE".to_string(),
            LlmProvider::Other(ref s) => format!("OTHER:{}", s),
        })
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
        let row = sqlx::query("SELECT * FROM user_settings WHERE user_id = ?")
            .bind(user_id.0.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(map_storage_err)?;

        Ok(row
            .map(|row| -> Result<UserSettings> {
                let provider_str: String = row.try_get("provider")?;
                let provider = if provider_str == "LOCAL" {
                    LlmProvider::Local
                } else if provider_str == "OPEN_AI_COMPATIBLE" {
                    LlmProvider::OpenAiCompatible
                } else if let Some(rest) = provider_str.strip_prefix("OTHER:") {
                    LlmProvider::Other(rest.to_string())
                } else {
                    LlmProvider::Local
                };

                let automation: AutomationSettings =
                    serde_json::from_str(row.try_get::<String, _>("automation")?.as_str())?;
                let weekly_review: WeeklyReviewSettings =
                    serde_json::from_str(row.try_get::<String, _>("weekly_review")?.as_str())?;

                Ok(UserSettings {
                    user_id,
                    provider,
                    model_for_planning: row.try_get("model_for_planning")?,
                    model_for_routine: row.try_get("model_for_routine")?,
                    automation,
                    weekly_review,
                })
            })
            .transpose()
            .map_err(map_storage_err)?)
    }
}
