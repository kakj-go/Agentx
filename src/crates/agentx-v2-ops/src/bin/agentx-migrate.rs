use agentx_v2_ops::{Plane, migrate};
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    migrate(Plane::parse(std::env::args().nth(1))?).await
}
