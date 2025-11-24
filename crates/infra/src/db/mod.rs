use std::path::{Path, PathBuf};

use anyhow::Result;
use sqlx::{SqlitePool, migrate::MigrateDatabase, sqlite::SqliteConnectOptions};

mod repositories;

pub use repositories::{
    SqliteListRepository, SqliteMilestoneRepository, SqliteProjectRepository,
    SqliteStatusRepository, SqliteTaskRepository, SqliteUserSettingsRepository,
};

/// Vault represents a workspace root that owns a SQLite database.
#[derive(Clone)]
pub struct Vault {
    pub root: PathBuf,
    pub pool: SqlitePool,
}

impl Vault {
    pub async fn connect_or_init(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        std::fs::create_dir_all(&root)?;

        let db_path = root.join("tasks.sqlite");
        let db_url = format!("sqlite:{}", db_path.to_string_lossy());

        if !sqlx::Sqlite::database_exists(&db_url)
            .await
            .unwrap_or(false)
        {
            sqlx::Sqlite::create_database(&db_url).await?;
        }

        // Use WAL for better concurrency on desktop apps.
        let options = SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);
        let pool = SqlitePool::connect_with(options).await?;

        sqlx::migrate!("./migrations").run(&pool).await?;

        Ok(Self { root, pool })
    }

    pub fn task_repo(&self) -> SqliteTaskRepository {
        SqliteTaskRepository::new(self.pool.clone())
    }

    pub fn project_repo(&self) -> SqliteProjectRepository {
        SqliteProjectRepository::new(self.pool.clone())
    }

    pub fn list_repo(&self) -> SqliteListRepository {
        SqliteListRepository::new(self.pool.clone())
    }

    pub fn milestone_repo(&self) -> SqliteMilestoneRepository {
        SqliteMilestoneRepository::new(self.pool.clone())
    }

    pub fn status_repo(&self) -> SqliteStatusRepository {
        SqliteStatusRepository::new(self.pool.clone())
    }

    pub fn user_settings_repo(&self) -> SqliteUserSettingsRepository {
        SqliteUserSettingsRepository::new(self.pool.clone())
    }
}
