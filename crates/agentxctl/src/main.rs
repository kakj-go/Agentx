#[tokio::main]
async fn main() {
    if let Err(error) = agentxctl::run().await {
        eprintln!(
            "agentxctl: {}",
            agentxctl::process::redact(&format!("{error:#}"))
        );
        std::process::exit(1);
    }
}
