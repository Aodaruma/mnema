use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Result, anyhow};
use mnema_core::prelude::{
    ListRepository, MilestoneRepository, ProjectRepository, ScheduleBlockRepository,
    StatusRepository, TaskRepository, UserSettingsRepository,
};
use sqlx::{
    PgPool, Postgres, SqlitePool, migrate::MigrateDatabase, postgres::PgPoolOptions,
    sqlite::SqliteConnectOptions,
};

mod defaults;
mod repositories;

pub use repositories::{
    PostgresListRepository, PostgresMilestoneRepository, PostgresProjectRepository,
    PostgresScheduleBlockRepository, PostgresStatusRepository, PostgresTaskRepository,
    PostgresUserSettingsRepository, SqliteListRepository, SqliteMilestoneRepository,
    SqliteProjectRepository, SqliteScheduleBlockRepository, SqliteStatusRepository,
    SqliteTaskRepository, SqliteUserSettingsRepository,
};

const DEFAULT_DATABASE_URL: &str = "postgres://postgres:postgres@localhost/mnema";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageBackend {
    Sqlite,
    Postgres,
}

impl StorageBackend {
    pub fn from_env() -> Result<Self> {
        match std::env::var("MNEMA_STORAGE_BACKEND") {
            Ok(value) if value.eq_ignore_ascii_case("postgres") => Ok(Self::Postgres),
            Ok(value) if value.eq_ignore_ascii_case("postgresql") => Ok(Self::Postgres),
            Ok(value) if value.eq_ignore_ascii_case("sqlite") => Ok(Self::Sqlite),
            Ok(value) => Err(anyhow!(
                "unsupported MNEMA_STORAGE_BACKEND value: {value}; use sqlite or postgres"
            )),
            Err(_) => Ok(Self::Sqlite),
        }
    }
}

#[derive(Clone)]
enum VaultDatabase {
    Sqlite(SqlitePool),
    Postgres(PgPool),
}

/// Vault represents a workspace root plus a configured database backend.
#[derive(Clone)]
pub struct Vault {
    pub root: PathBuf,
    database: VaultDatabase,
}

impl Vault {
    pub async fn connect_or_init(root: impl AsRef<Path>) -> Result<Self> {
        match StorageBackend::from_env()? {
            StorageBackend::Sqlite => {
                let sqlite_path = std::env::var("MNEMA_SQLITE_PATH")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| default_sqlite_path());
                Self::connect_or_init_with_sqlite_path(root, sqlite_path).await
            }
            StorageBackend::Postgres => {
                let database_url = std::env::var("MNEMA_DATABASE_URL")
                    .unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string());
                Self::connect_or_init_with_database_url(root, &database_url).await
            }
        }
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

        sqlx::migrate!("./migrations/postgres").run(&pool).await?;

        Ok(Self {
            root,
            database: VaultDatabase::Postgres(pool),
        })
    }

    pub async fn connect_or_init_with_sqlite_path(
        root: impl AsRef<Path>,
        sqlite_path: impl AsRef<Path>,
    ) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        std::fs::create_dir_all(&root)?;

        let sqlite_path = sqlite_path.as_ref().to_path_buf();
        if let Some(parent) = sqlite_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let options = SqliteConnectOptions::new()
            .filename(&sqlite_path)
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .foreign_keys(true);
        let pool = SqlitePool::connect_with(options).await?;

        sqlx::migrate!("./migrations/sqlite").run(&pool).await?;

        Ok(Self {
            root,
            database: VaultDatabase::Sqlite(pool),
        })
    }

    pub fn backend(&self) -> StorageBackend {
        match &self.database {
            VaultDatabase::Sqlite(_) => StorageBackend::Sqlite,
            VaultDatabase::Postgres(_) => StorageBackend::Postgres,
        }
    }

    /// デフォルトのステータス/リストを投入するヘルパー。
    pub async fn initialize_defaults(&self) -> Result<()> {
        match &self.database {
            VaultDatabase::Sqlite(pool) => defaults::initialize_defaults_sqlite(pool).await,
            VaultDatabase::Postgres(pool) => defaults::initialize_defaults_postgres(pool).await,
        }
    }

    pub fn task_repo(&self) -> TaskRepo {
        match &self.database {
            VaultDatabase::Sqlite(pool) => Arc::new(SqliteTaskRepository::new(pool.clone())),
            VaultDatabase::Postgres(pool) => Arc::new(PostgresTaskRepository::new(pool.clone())),
        }
    }

    pub fn project_repo(&self) -> ProjectRepo {
        match &self.database {
            VaultDatabase::Sqlite(pool) => Arc::new(SqliteProjectRepository::new(pool.clone())),
            VaultDatabase::Postgres(pool) => Arc::new(PostgresProjectRepository::new(pool.clone())),
        }
    }

    pub fn list_repo(&self) -> ListRepo {
        match &self.database {
            VaultDatabase::Sqlite(pool) => Arc::new(SqliteListRepository::new(pool.clone())),
            VaultDatabase::Postgres(pool) => Arc::new(PostgresListRepository::new(pool.clone())),
        }
    }

    pub fn milestone_repo(&self) -> MilestoneRepo {
        match &self.database {
            VaultDatabase::Sqlite(pool) => Arc::new(SqliteMilestoneRepository::new(pool.clone())),
            VaultDatabase::Postgres(pool) => {
                Arc::new(PostgresMilestoneRepository::new(pool.clone()))
            }
        }
    }

    pub fn status_repo(&self) -> StatusRepo {
        match &self.database {
            VaultDatabase::Sqlite(pool) => Arc::new(SqliteStatusRepository::new(pool.clone())),
            VaultDatabase::Postgres(pool) => Arc::new(PostgresStatusRepository::new(pool.clone())),
        }
    }

    pub fn schedule_block_repo(&self) -> ScheduleBlockRepo {
        match &self.database {
            VaultDatabase::Sqlite(pool) => {
                Arc::new(SqliteScheduleBlockRepository::new(pool.clone()))
            }
            VaultDatabase::Postgres(pool) => {
                Arc::new(PostgresScheduleBlockRepository::new(pool.clone()))
            }
        }
    }

    pub fn user_settings_repo(&self) -> UserSettingsRepo {
        match &self.database {
            VaultDatabase::Sqlite(pool) => {
                Arc::new(SqliteUserSettingsRepository::new(pool.clone()))
            }
            VaultDatabase::Postgres(pool) => {
                Arc::new(PostgresUserSettingsRepository::new(pool.clone()))
            }
        }
    }
}

fn default_sqlite_path() -> PathBuf {
    if let Ok(path) = std::env::var("LOCALAPPDATA") {
        return PathBuf::from(path).join("Mnema").join("mnema.sqlite");
    }
    if let Ok(path) = std::env::var("XDG_DATA_HOME") {
        return PathBuf::from(path).join("mnema").join("mnema.sqlite");
    }
    if let Ok(path) = std::env::var("HOME") {
        return PathBuf::from(path)
            .join(".local")
            .join("share")
            .join("mnema")
            .join("mnema.sqlite");
    }
    PathBuf::from("./mnema.sqlite")
}

pub type TaskRepo = Arc<dyn TaskRepository>;
pub type ProjectRepo = Arc<dyn ProjectRepository>;
pub type ListRepo = Arc<dyn ListRepository>;
pub type MilestoneRepo = Arc<dyn MilestoneRepository>;
pub type StatusRepo = Arc<dyn StatusRepository>;
pub type ScheduleBlockRepo = Arc<dyn ScheduleBlockRepository>;
pub type UserSettingsRepo = Arc<dyn UserSettingsRepository>;
