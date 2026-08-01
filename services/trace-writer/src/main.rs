#[tokio::main]
async fn main() -> anyhow::Result<()> {
    agentx_service_kit::run_service("trace-writer").await
}
