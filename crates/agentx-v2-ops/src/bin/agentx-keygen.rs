use anyhow::Result;

fn main() -> Result<()> {
    println!(
        "{}",
        serde_json::to_string(&agentx_key_material::generate()?)?
    );
    Ok(())
}
