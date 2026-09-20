#[tokio::main]
async fn main() -> anyhow::Result<()> {
    mnema_server::run().await
}
