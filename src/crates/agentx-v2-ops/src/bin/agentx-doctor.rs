use agentx_v2_ops::{Plane, doctor};
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    doctor(Plane::parse(std::env::args().nth(1))?).await
}
