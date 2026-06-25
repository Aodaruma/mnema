use std::path::{Path, PathBuf};

use anyhow::Result;
use sqlx::{PgPool, Postgres, migrate::MigrateDatabase, postgres::PgPoolOptions};

mod defaults;

mod repositories;

pub use repositories::{
    PostgresListRepository, PostgresMilestoneRepository, PostgresProjectRepository,
    PostgresScheduleBlockRepository, PostgresStatusRepository, PostgresTaskRepository,
    PostgresUserSettingsRepository,
};

const DEFAULT_DATABASE_URL: &str = "postgres://postgres:postgres@localhost/mnema";

/// Vault represents a workspace root plus its PostgreSQL-backed data store.
#[derive(Clone)]
pub struct Vault {
    pub root: PathBuf,
    pub pool: PgPool,
}

impl Vault {
    pub async fn connect_or_init(root: impl AsRef<Path>) -> Result<Self> {
        let database_url = std::env::var("MNEMA_DATABASE_URL")
            .unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string());
        Self::connect_or_init_with_database_url(root, &database_url).await
    }

    pub async fn connect_or_init_with_database_url(
        root: impl AsRef<Path>,
        database_url: &str,
    ) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        std::fs::create_dir_all(&root)?;

        if !Postgres::database_exists(database_url)
            .await
            .unwrap_or(false)
        {
            Postgres::create_database(database_url).await?;
        }

        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await?;

        sqlx::migrate!("./migrations").run(&pool).await?;

        Ok(Self { root, pool })
    }

    /// デフォルトのステータス/リストを投入するヘルパー。
    pub async fn initialize_defaults(&self) -> Result<()> {
        defaults::initialize_defaults(&self.pool).await
    }

    pub fn task_repo(&self) -> PostgresTaskRepository {
        PostgresTaskRepository::new(self.pool.clone())
    }

    pub fn project_repo(&self) -> PostgresProjectRepository {
        PostgresProjectRepository::new(self.pool.clone())
    }

    pub fn list_repo(&self) -> PostgresListRepository {
        PostgresListRepository::new(self.pool.clone())
    }

    pub fn milestone_repo(&self) -> PostgresMilestoneRepository {
        PostgresMilestoneRepository::new(self.pool.clone())
    }

    pub fn status_repo(&self) -> PostgresStatusRepository {
        PostgresStatusRepository::new(self.pool.clone())
    }

    pub fn schedule_block_repo(&self) -> PostgresScheduleBlockRepository {
        PostgresScheduleBlockRepository::new(self.pool.clone())
    }

    pub fn user_settings_repo(&self) -> PostgresUserSettingsRepository {
        PostgresUserSettingsRepository::new(self.pool.clone())
    }
}
