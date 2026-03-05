use anyhow::Result;
use mnema_core::prelude::*;
use sqlx::{Acquire, SqlitePool};

/// ステータス/リストのデフォルトデータを投入する。
pub async fn initialize_defaults(pool: &SqlitePool) -> Result<()> {
    let mut conn = pool.acquire().await?;
    let mut tx = conn.begin().await?;

    // Status groups
    let groups = vec![
        (StatusGroupKind::NotStarted, "Not started"),
        (StatusGroupKind::InProgress, "In progress"),
        (StatusGroupKind::Pending, "Pending"),
        (StatusGroupKind::Done, "Done"),
    ];
    for (kind, name) in groups {
        sqlx::query(r#"INSERT OR IGNORE INTO status_groups (id, name, kind) VALUES (?, ?, ?)"#)
            .bind(StatusGroupId::new().0.to_string())
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
        ("To do", StatusGroupKind::NotStarted, 0),
        ("In progress", StatusGroupKind::InProgress, 1),
        ("Pending", StatusGroupKind::Pending, 2),
        ("Done", StatusGroupKind::Done, 3),
    ];
    for (name, kind, order) in statuses {
        // fetch group id by kind
        let group_id: Option<String> =
            sqlx::query_scalar("SELECT id FROM status_groups WHERE kind = ? LIMIT 1")
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
                r#"INSERT OR IGNORE INTO statuses (id, project_id, name, group_id, "order")
                   VALUES (?, NULL, ?, ?, ?)"#,
            )
            .bind(StatusId::new().0.to_string())
            .bind(name)
            .bind(gid)
            .bind(order)
            .execute(&mut *tx)
            .await?;
        }
    }

    // System lists
    let lists = vec![
        ("Inbox", ListKind::Inbox, 0),
        ("Personal", ListKind::Personal, 1),
    ];
    for (name, kind, order) in lists {
        sqlx::query(
            r#"INSERT OR IGNORE INTO lists (id, project_id, name, is_system, kind, view_type, "order")
               VALUES (?, NULL, ?, 1, ?, 'LIST', ?)"#,
        )
        .bind(ListId::new().0.to_string())
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
