// crates/desktop/src/main.rs
// Phase5 スパイク: Vault を開いて起動確認するだけの CLI プレースホルダー。

use mnema_infra::db::Vault;
use std::env;
use std::path::PathBuf;
use time::OffsetDateTime;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let vault_path = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("./vault"));

    println!("📂 Vault: {}", vault_path.display());
    let vault = Vault::connect_or_init(&vault_path).await?;

    // ここでは UI がまだ無いので、起動確認のメッセージのみ。
    let _today = OffsetDateTime::now_utc().date();
    let _tasks_repo = vault.task_repo();

    println!("Mnema desktop stub: 起動しました（UI 実装前）。");
    Ok(())
}
