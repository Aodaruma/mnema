use anyhow::Result;
use mnema_core::prelude::*;
use sqlx::{Acquire, PgPool};
use uuid::Uuid;

fn uuid(value: &str) -> Uuid {
    Uuid::parse_str(value).expect("default UUID constants must be valid")
}

/// ステータス/リストのデフォルトデータを投入する。
pub async fn initialize_defaults(pool: &PgPool) -> Result<()> {
    let mut conn = pool.acquire().await?;
    let mut tx = conn.begin().await?;

    // Status groups
    let groups = vec![
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
    ];
    for (id, kind, name) in groups {
        sqlx::query(
            r#"
            INSERT INTO status_groups (id, name, kind)
            VALUES ($1, $2, $3)
            ON CONFLICT (id) DO NOTHING
        "#,
        )
        .bind(id)
        .bind(name)
        .bind(match kind {
            StatusGroupKind::NotStarted => "NOT_STARTED",
            StatusGroupKind::InProgress => "IN_PROGRESS",
            StatusGroupKind::Pending => "PENDING",
            StatusGroupKind::Done => "DONE",
        })
        .execute(&mut *tx)
        .await?;
    }

    // Statuses (global)
    let statuses = vec![
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
    ];
    for (id, name, kind, order) in statuses {
        // fetch group id by kind
        let group_id: Option<Uuid> =
            sqlx::query_scalar("SELECT id FROM status_groups WHERE kind = $1 LIMIT 1")
                .bind(match kind {
                    StatusGroupKind::NotStarted => "NOT_STARTED",
                    StatusGroupKind::InProgress => "IN_PROGRESS",
                    StatusGroupKind::Pending => "PENDING",
                    StatusGroupKind::Done => "DONE",
                })
                .fetch_optional(&mut *tx)
                .await?;

        if let Some(gid) = group_id {
            sqlx::query(
                r#"
                INSERT INTO statuses (id, project_id, name, group_id, "order")
                VALUES ($1, NULL, $2, $3, $4)
                ON CONFLICT (id) DO NOTHING
            "#,
            )
            .bind(id)
            .bind(name)
            .bind(gid)
            .bind(order)
            .execute(&mut *tx)
            .await?;
        }
    }

    // System lists
    let lists = vec![
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
    ];
    for (id, name, kind, order) in lists {
        sqlx::query(
            r#"
            INSERT INTO lists (id, project_id, name, is_system, kind, view_type, "order")
            VALUES ($1, NULL, $2, TRUE, $3, 'LIST', $4)
            ON CONFLICT (id) DO NOTHING
        "#,
        )
        .bind(id)
        .bind(name)
        .bind(match kind {
            ListKind::Inbox => "INBOX",
            ListKind::Personal => "PERSONAL",
            ListKind::Project => "PROJECT",
        })
        .bind(order)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}
