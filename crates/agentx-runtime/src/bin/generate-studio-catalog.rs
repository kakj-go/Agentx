use std::path::PathBuf;

use agentx_runtime::NodeRegistry;

fn main() -> anyhow::Result<()> {
    let mut arguments = std::env::args().skip(1);
    let output = PathBuf::from(
        arguments
            .next()
            .ok_or_else(|| anyhow::anyhow!("usage: generate-studio-catalog <output-file>"))?,
    );
    anyhow::ensure!(arguments.next().is_none(), "unexpected extra arguments");
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let registry = NodeRegistry::m5_defaults();
    let manifests = registry.studio_manifests().collect::<Vec<_>>();
    std::fs::write(output, serde_json::to_string_pretty(&manifests)? + "\n")?;
    Ok(())
}
