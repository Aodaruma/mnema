use mnema_core::prelude::*;
use mnema_infra::db::Vault;
use tempfile::tempdir;
use time::{Date, OffsetDateTime};

fn utc_now() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}

fn today() -> Date {
    utc_now().date()
}

async fn default_status(vault: &Vault) -> Status {
    let group = StatusGroup {
        id: StatusGroupId::new(),
        name: "Not started".into(),
        kind: StatusGroupKind::NotStarted,
    };
    let group_id = group.id.clone();
    let status = Status {
        id: StatusId::new(),
        project_id: None,
        name: "Todo".into(),
        group_id,
        order: 0,
    };
    let repo = vault.status_repo();
    repo.insert_group(group).await.unwrap();
    repo.insert_status(status.clone()).await.unwrap();
    status
}

#[tokio::test]
async fn task_crud_roundtrip() -> anyhow::Result<()> {
    let dir = tempdir()?;
    let vault = Vault::connect_or_init(dir.path()).await?;

    let status = default_status(&vault).await;

    let project = Project {
        id: ProjectId::new(),
        title: "Phase 1".into(),
        description: Some("test project".into()),
        start_date: Some(today()),
        end_date: None,
        default_status_set_id: None,
        archived_at: None,
    };
    let project_id = project.id.clone();
    vault.project_repo().insert(project.clone()).await?;

    let list = List {
        id: ListId::new(),
        project_id: Some(project_id.clone()),
        name: "Main".into(),
        is_system: false,
        kind: ListKind::Project,
        view_type: ListViewType::List,
        order: 0,
    };
    vault.list_repo().insert(list.clone()).await?;

    let created_at = utc_now();
    let mut task = Task {
        id: TaskId::new(),
        title: "Write code".into(),
        description: Some("implement phase 2".into()),
        project_id: Some(project_id.clone()),
        list_id: Some(list.id),
        status_id: status.id,
        due_date: Some(today()),
        start_date: Some(today()),
        estimated_minutes: Some(120),
        cost_points: Some(3),
        dependencies: vec![],
        milestone_id: None,
        created_at,
        updated_at: created_at,
        deleted_at: None,
    };

    let repo = vault.task_repo();
    repo.insert(task.clone()).await?;

    let task_id = task.id.clone();

    let fetched = repo.find(task_id.clone()).await?;
    assert_eq!(Some(task.clone()), fetched);

    task.title = "Write code (updated)".into();
    task.updated_at = utc_now();
    repo.update(task.clone()).await?;

    let fetched = repo.find(task_id).await?.unwrap();
    assert_eq!("Write code (updated)", fetched.title);

    Ok(())
}

#[tokio::test]
async fn initialize_defaults_creates_statuses_and_lists() -> anyhow::Result<()> {
    let dir = tempdir()?;
    let vault = Vault::connect_or_init(dir.path()).await?;
    vault.initialize_defaults().await?;

    let status_repo = vault.status_repo();
    let list_repo = vault.list_repo();

    let statuses = status_repo.list_statuses_for_project(None).await?;
    assert!(statuses.len() >= 4);

    let system_lists = list_repo.list_system().await?;
    assert!(system_lists.iter().any(|l| l.kind == ListKind::Inbox));
    assert!(system_lists.iter().any(|l| l.kind == ListKind::Personal));
    Ok(())
}

#[tokio::test]
async fn user_settings_upsert_and_get() -> anyhow::Result<()> {
    let dir = tempdir()?;
    let vault = Vault::connect_or_init(dir.path()).await?;
    let repo = vault.user_settings_repo();

    let mut settings = UserSettings::default();
    settings.provider = LlmProvider::OpenAiCompatible;
    settings.model_for_planning = Some("gpt-4.1".into());

    repo.upsert(settings.clone()).await?;
    let user_id = settings.user_id.clone();

    let stored = repo.get(user_id.clone()).await?.expect("stored");

    assert_eq!(settings.provider, stored.provider);
    assert_eq!(settings.model_for_planning, stored.model_for_planning);

    settings.model_for_planning = Some("gpt-4.1-mini".into());
    repo.upsert(settings.clone()).await?;
    let stored_again = repo.get(user_id).await?.expect("stored");
    assert_eq!(stored_again.model_for_planning, settings.model_for_planning);

    Ok(())
}
