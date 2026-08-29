use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::Value;

const CONTRACT: &str = include_str!("../../../openapi/platform-api.json");

pub(crate) fn requested_output() -> Result<Option<PathBuf>> {
    let mut arguments = std::env::args().skip(1);
    let Some(command) = arguments.next() else {
        return Ok(None);
    };
    if command != "openapi" {
        anyhow::bail!("unknown platform-control command {command}");
    }
    Ok(Some(PathBuf::from(
        arguments
            .next()
            .unwrap_or_else(|| "openapi/platform-api.json".into()),
    )))
}

pub(crate) fn write(path: &Path) -> Result<()> {
    let mut contract: Value = serde_json::from_str(CONTRACT).context("invalid Platform OpenAPI")?;
    contract["x-agentx-generator"] = Value::String("platform-control".into());
    contract["x-agentx-contract-version"] = Value::String("v2-08a".into());
    let value = serde_json::to_string_pretty(&contract)? + "\n";
    std::fs::write(path, value)
        .with_context(|| format!("write Platform OpenAPI to {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_contract_has_all_cutover_paths() {
        let value: Value = serde_json::from_str(CONTRACT).unwrap();
        assert_eq!(value["paths"].as_object().unwrap().len(), 157);
        assert!(value["paths"]["/api/v1/workflows/{id}/run"]["post"].is_object());
        assert!(value["paths"]["/api/v1/runtime/quotas"]["put"].is_object());
        assert!(value["paths"]["/api/v1/departments/{id}/resource-options"]["get"].is_object());
    }
}
