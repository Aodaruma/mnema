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

async fn test_vault() -> anyhow::Result<Option<Vault>> {
    let Ok(database_url) = std::env::var("MNEMA_TEST_DATABASE_URL") else {
        eprintln!("skipping PostgreSQL integration test: MNEMA_TEST_DATABASE_URL is not set");
        return Ok(None);
    };
    let dir = tempdir()?;
    Vault::connect_or_init_with_database_url(dir.path(), &database_url)
        .await
        .map(Some)
}

async fn default_status(vault: &Vault) -> anyhow::Result<Status> {
    vault.initialize_defaults().await?;
    let statuses = vault.status_repo().list_statuses_for_project(None).await?;
    statuses
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("default status was not created"))
}

#[tokio::test]
async fn task_crud_roundtrip() -> anyhow::Result<()> {
    let Some(vault) = test_vault().await? else {
        return Ok(());
    };

    let status = default_status(&vault).await?;

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
    let Some(vault) = test_vault().await? else {
        return Ok(());
    };
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
    let Some(vault) = test_vault().await? else {
        return Ok(());
    };
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

#[tokio::test]
async fn schedule_block_replace_and_list_for_day() -> anyhow::Result<()> {
    let Some(vault) = test_vault().await? else {
        return Ok(());
    };
    let repo = vault.schedule_block_repo();
    let target_date = today();
    let start_at = target_date.with_hms(9, 0, 0)?.assume_utc();
    let end_at = target_date.with_hms(9, 45, 0)?.assume_utc();
    let now = utc_now();
    let block = ScheduleBlock {
        id: ScheduleBlockId::new(),
        task_id: None,
        title_snapshot: Some("Proposed block".into()),
        start_at,
        end_at,
        block_type: ScheduleBlockType::Task,
        state: ScheduleBlockState::Proposed,
        locked: false,
        source: ScheduleBlockSource::Scheduler,
        required_minutes: Some(45),
        created_at: now,
        updated_at: now,
    };

    repo.replace_proposed_for_day(target_date, vec![block.clone()])
        .await?;
    let blocks = repo.list_for_day(target_date).await?;
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].title_snapshot, block.title_snapshot);

    repo.replace_proposed_for_day(target_date, Vec::new())
        .await?;
    let blocks = repo.list_for_day(target_date).await?;
    assert!(blocks.is_empty());

    Ok(())
}
