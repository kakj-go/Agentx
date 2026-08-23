use anyhow::{Context, Result};
use include_dir::{Dir, include_dir};
use std::path::{Path, PathBuf};
use tempfile::TempDir;

static CHARTS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../deploy/helm");
pub const VALUES_SCHEMA: &str = include_str!("../../../deploy/values/values.schema.json");
pub const RELEASE_SCHEMA: &str =
    include_str!("../../../deploy/release/v2-release-manifest.schema.json");
pub const BACKUP_SCHEMA: &str = include_str!("../../../deploy/release/backup-manifest.schema.json");
const INGRESS_VALUES: &str = include_str!("../../../deploy/ingress-nginx/values.yaml");
const INGRESS_CHART: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ingress-nginx-4.15.1.tgz"));

pub struct EmbeddedAssets {
    _temp: TempDir,
    root: PathBuf,
}

impl EmbeddedAssets {
    pub fn extract() -> Result<Self> {
        let temp = tempfile::tempdir().context("create embedded deployment asset directory")?;
        let root = temp.path().to_path_buf();
        let charts = root.join("helm");
        CHARTS
            .extract(&charts)
            .context("extract embedded Helm charts")?;
        std::fs::create_dir_all(root.join("ingress-nginx"))?;
        std::fs::write(root.join("ingress-nginx/values.yaml"), INGRESS_VALUES)?;
        std::fs::write(
            root.join("ingress-nginx/ingress-nginx-4.15.1.tgz"),
            INGRESS_CHART,
        )?;
        Ok(Self { _temp: temp, root })
    }

    pub fn chart(&self, target: &str) -> PathBuf {
        self.root.join("helm").join(format!("agentx-{target}"))
    }

    pub fn ingress_chart(&self) -> PathBuf {
        self.root.join("ingress-nginx/ingress-nginx-4.15.1.tgz")
    }

    pub fn ingress_values(&self) -> PathBuf {
        self.root.join("ingress-nginx/values.yaml")
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compare_directories(source: &Path, embedded: &Path) {
        let mut source_entries = std::fs::read_dir(source)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        let mut embedded_entries = std::fs::read_dir(embedded)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        source_entries.sort();
        embedded_entries.sort();
        assert_eq!(source_entries, embedded_entries, "{}", source.display());
        for name in source_entries {
            let source = source.join(&name);
            let embedded = embedded.join(name);
            if source.is_dir() {
                compare_directories(&source, &embedded);
            } else {
                assert_eq!(
                    std::fs::read(&source).unwrap(),
                    std::fs::read(&embedded).unwrap()
                );
            }
        }
    }

    #[test]
    fn extracted_assets_match_the_bound_repository_sources() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let assets = EmbeddedAssets::extract().unwrap();
        compare_directories(&root.join("deploy/helm"), &assets.root().join("helm"));
        assert_eq!(
            std::fs::read(root.join("deploy/ingress-nginx/values.yaml")).unwrap(),
            std::fs::read(assets.ingress_values()).unwrap()
        );
        let ingress = std::fs::read(assets.ingress_chart()).unwrap();
        assert!(ingress.len() > 1024);
        assert_eq!(&ingress[..2], &[0x1f, 0x8b]);
    }

    #[test]
    fn vault_bootstrap_looks_up_fixed_tokens_as_positional_arguments() {
        let assets = EmbeddedAssets::extract().unwrap();
        let template = std::fs::read_to_string(
            assets
                .chart("dependencies")
                .join("templates/job-vault-bootstrap.yaml"),
        )
        .unwrap();

        for variable in ["CONTROL_VAULT_TOKEN", "RUNTIME_VAULT_TOKEN"] {
            assert!(
                template.contains(&format!("vault token lookup -- \"${variable}\"")),
                "{variable} lookup must accept token values beginning with '-'"
            );
        }
    }

    #[test]
    fn observability_schema_gate_selects_the_configured_database() {
        let assets = EmbeddedAssets::extract().unwrap();
        let helpers =
            std::fs::read_to_string(assets.chart("observability").join("templates/_helpers.tpl"))
                .unwrap();

        assert!(helpers.contains("--data-urlencode \"database=$AGENTX_CLICKHOUSE_DATABASE\""));
        assert!(helpers.contains("name: AGENTX_CLICKHOUSE_DATABASE"));
    }
}
