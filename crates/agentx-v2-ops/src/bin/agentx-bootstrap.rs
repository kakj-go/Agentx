use agentx_v2_ops::{Plane, bootstrap};
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    bootstrap(Plane::parse(std::env::args().nth(1))?).await
}
