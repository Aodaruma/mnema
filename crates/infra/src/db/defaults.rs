use anyhow::Result;
use mnema_core::prelude::*;
use sqlx::{Acquire, PgPool, SqlitePool};
use uuid::Uuid;

fn uuid(value: &str) -> Uuid {
    Uuid::parse_str(value).expect("default UUID constants must be valid")
}

fn status_group_kind_to_str(kind: StatusGroupKind) -> &'static str {
    match kind {
        StatusGroupKind::NotStarted => "NOT_STARTED",
        StatusGroupKind::InProgress => "IN_PROGRESS",
        StatusGroupKind::Pending => "PENDING",
        StatusGroupKind::Done => "DONE",
    }
}

fn list_kind_to_str(kind: ListKind) -> &'static str {
    match kind {
        ListKind::Inbox => "INBOX",
        ListKind::Personal => "PERSONAL",
        ListKind::Project => "PROJECT",
    }
}

/// ステータス/リストのデフォルトデータを PostgreSQL に投入する。
pub async fn initialize_defaults_postgres(pool: &PgPool) -> Result<()> {
    let mut conn = pool.acquire().await?;
    let mut tx = conn.begin().await?;

    for (id, kind, name) in default_status_groups() {
        sqlx::query(
            r#"
            INSERT INTO status_groups (id, name, kind)
            VALUES ($1, $2, $3)
            ON CONFLICT (id) DO NOTHING
        "#,
        )
        .bind(id)
        .bind(name)
        .bind(status_group_kind_to_str(kind))
        .execute(&mut *tx)
        .await?;
    }

    for (id, name, kind, order) in default_statuses() {
        let group_id: Option<Uuid> =
            sqlx::query_scalar("SELECT id FROM status_groups WHERE kind = $1 LIMIT 1")
                .bind(status_group_kind_to_str(kind))
                .fetch_optional(&mut *tx)
                .await?;

        if let Some(group_id) = group_id {
            sqlx::query(
                r#"
                INSERT INTO statuses (id, project_id, name, group_id, "order")
                VALUES ($1, NULL, $2, $3, $4)
                ON CONFLICT (id) DO NOTHING
            "#,
            )
            .bind(id)
            .bind(name)
            .bind(group_id)
            .bind(order)
            .execute(&mut *tx)
            .await?;
        }
    }

    for (id, name, kind, order) in default_lists() {
        sqlx::query(
            r#"
            INSERT INTO lists (id, project_id, name, is_system, kind, view_type, "order")
            VALUES ($1, NULL, $2, TRUE, $3, 'LIST', $4)
            ON CONFLICT (id) DO NOTHING
        "#,
        )
        .bind(id)
        .bind(name)
        .bind(list_kind_to_str(kind))
        .bind(order)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}

/// ステータス/リストのデフォルトデータを SQLite に投入する。
pub async fn initialize_defaults_sqlite(pool: &SqlitePool) -> Result<()> {
    let mut tx = pool.begin().await?;

    for (id, kind, name) in default_status_groups() {
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO status_groups (id, name, kind)
            VALUES (?, ?, ?)
        "#,
        )
        .bind(id.to_string())
        .bind(name)
        .bind(status_group_kind_to_str(kind))
        .execute(&mut *tx)
        .await?;
    }

    for (id, name, kind, order) in default_statuses() {
        let group_id: Option<String> =
            sqlx::query_scalar("SELECT id FROM status_groups WHERE kind = ? LIMIT 1")
                .bind(status_group_kind_to_str(kind))
                .fetch_optional(&mut *tx)
                .await?;

        if let Some(group_id) = group_id {
            sqlx::query(
                r#"
                INSERT OR IGNORE INTO statuses (id, project_id, name, group_id, "order")
                VALUES (?, NULL, ?, ?, ?)
            "#,
            )
            .bind(id.to_string())
            .bind(name)
            .bind(group_id)
            .bind(order)
            .execute(&mut *tx)
            .await?;
        }
    }

    for (id, name, kind, order) in default_lists() {
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO lists (id, project_id, name, is_system, kind, view_type, "order")
            VALUES (?, NULL, ?, 1, ?, 'LIST', ?)
        "#,
        )
        .bind(id.to_string())
        .bind(name)
        .bind(list_kind_to_str(kind))
        .bind(order)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}

fn default_status_groups() -> Vec<(Uuid, StatusGroupKind, &'static str)> {
    vec![
        (
            uuid("00000000-0000-0000-0000-000000000101"),
            StatusGroupKind::NotStarted,
            "Not started",
        ),
        (
            uuid("00000000-0000-0000-0000-000000000102"),
            StatusGroupKind::InProgress,
            "In progress",
        ),
        (
            uuid("00000000-0000-0000-0000-000000000103"),
            StatusGroupKind::Pending,
            "Pending",
        ),
        (
            uuid("00000000-0000-0000-0000-000000000104"),
            StatusGroupKind::Done,
            "Done",
        ),
    ]
}

fn default_statuses() -> Vec<(Uuid, &'static str, StatusGroupKind, i32)> {
    vec![
        (
            uuid("00000000-0000-0000-0000-000000000201"),
            "To do",
            StatusGroupKind::NotStarted,
            0,
        ),
        (
            uuid("00000000-0000-0000-0000-000000000202"),
            "In progress",
            StatusGroupKind::InProgress,
            1,
        ),
        (
            uuid("00000000-0000-0000-0000-000000000203"),
            "Pending",
            StatusGroupKind::Pending,
            2,
        ),
        (
            uuid("00000000-0000-0000-0000-000000000204"),
            "Done",
            StatusGroupKind::Done,
            3,
        ),
    ]
}

fn default_lists() -> Vec<(Uuid, &'static str, ListKind, i32)> {
    vec![
        (
            uuid("00000000-0000-0000-0000-000000000301"),
            "Inbox",
            ListKind::Inbox,
            0,
        ),
        (
            uuid("00000000-0000-0000-0000-000000000302"),
            "Personal",
            ListKind::Personal,
            1,
        ),
    ]
}
