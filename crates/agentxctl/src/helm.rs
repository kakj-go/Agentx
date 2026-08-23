use crate::{assets::EmbeddedAssets, config::DeploymentConfig, process};
use anyhow::{Context, Result, bail};
use regex::Regex;
use serde_json::{Value, json};
use std::{collections::BTreeMap, io::Write, path::PathBuf};
use tempfile::NamedTempFile;

pub const INGRESS_RELEASE: &str = "agentx-ingress-nginx";

pub fn release_name(target: &str) -> &'static str {
    match target {
        "dependencies" => "agentx-dependencies",
        "control" => "agentx-control",
        "runtime" => "agentx-runtime",
        "observability" => "agentx-observability",
        _ => panic!("unsupported target"),
    }
}

pub struct Helm<'a> {
    pub config: &'a DeploymentConfig,
    pub assets: &'a EmbeddedAssets,
}

impl Helm<'_> {
    fn values_file(&self) -> Result<NamedTempFile> {
        let mut file = tempfile::Builder::new().suffix(".yaml").tempfile()?;
        file.write_all(self.config.to_yaml()?.as_bytes())?;
        file.flush()?;
        Ok(file)
    }

    pub async fn tool_versions(&self) -> Result<Value> {
        process::find_tool("helm")?;
        process::find_tool("kubectl")?;
        let helm = process::run_command(["helm", "version", "--short"], None, None, 30, true, None)
            .await?
            .stdout
            .trim()
            .to_owned();
        if !Regex::new(r"(?:^|\s)v?3\.").unwrap().is_match(&helm) {
            bail!("Helm 3 is required, found: {helm}");
        }
        let kubectl = process::run_command(
            ["kubectl", "version", "--client", "-o", "json"],
            None,
            None,
            30,
            true,
            None,
        )
        .await?
        .json()?;
        Ok(json!({
            "helm": helm,
            "kubectl": kubectl.pointer("/clientVersion/gitVersion").and_then(Value::as_str).unwrap_or("unknown"),
            "agentxctl": env!("CARGO_PKG_VERSION"),
        }))
    }

    pub async fn template(&self, target: &str) -> Result<String> {
        process::find_tool("helm")?;
        let values = self.values_file()?;
        let args = vec![
            "helm".into(),
            "template".into(),
            release_name(target).into(),
            path(self.assets.chart(target)),
            "--namespace".into(),
            self.config.namespace(target).into(),
            "--values".into(),
            path(values.path()),
        ];
        Ok(process::run_command(args, None, None, 120, true, None)
            .await?
            .stdout)
    }

    pub async fn lint(&self, targets: &[&str]) -> Result<()> {
        process::find_tool("helm")?;
        let values = self.values_file()?;
        for target in targets {
            let args = vec![
                "helm".into(),
                "lint".into(),
                path(self.assets.chart(target)),
                "--values".into(),
                path(values.path()),
            ];
            process::run_command(args, None, None, 120, true, None).await?;
        }
        Ok(())
    }

    pub async fn upgrade_install(
        &self,
        target: &str,
        set_values: &BTreeMap<String, u64>,
    ) -> Result<()> {
        let values = self.values_file()?;
        let mut args = vec![
            "helm".into(),
            "upgrade".into(),
            "--install".into(),
            release_name(target).into(),
            path(self.assets.chart(target)),
            "--namespace".into(),
            self.config.namespace(target).into(),
            "--values".into(),
            path(values.path()),
            "--atomic".into(),
            "--wait".into(),
            "--wait-for-jobs".into(),
            "--timeout".into(),
            "10m".into(),
        ];
        for (key, value) in set_values {
            args.extend(["--set".into(), format!("{key}={value}")]);
        }
        process::run_command(args, None, None, 720, true, None).await?;
        Ok(())
    }

    pub async fn install_ingress(&self) -> Result<()> {
        let ingress = self.config.object("/global/ingress").unwrap();
        let class_name = ingress.get("className").and_then(Value::as_str).unwrap();
        let mut args = vec![
            "helm".into(),
            "upgrade".into(),
            "--install".into(),
            INGRESS_RELEASE.into(),
            path(self.assets.ingress_chart()),
            "--namespace".into(),
            self.config.namespace("dependencies").into(),
            "--create-namespace".into(),
            "--atomic".into(),
            "--wait".into(),
            "--timeout".into(),
            "10m".into(),
            "-f".into(),
            path(self.assets.ingress_values()),
            "--set-string".into(),
            format!("controller.ingressClass={class_name}"),
            "--set-string".into(),
            format!("controller.ingressClassResource.name={class_name}"),
            "--set-string".into(),
            format!("controller.ingressClassResource.controllerValue=k8s.io/{class_name}"),
        ];
        if self.config.environment() == "test"
            || self
                .config
                .namespace("dependencies")
                .starts_with("agentx-e2e-")
        {
            args.extend([
                "--set-string".into(),
                format!("fullnameOverride={class_name}"),
                "--set-string".into(),
                "controller.service.type=ClusterIP".into(),
            ]);
        }
        process::run_command(args, None, None, 720, true, None).await?;
        Ok(())
    }

    pub async fn release_status(&self, target: &str) -> Result<Option<Value>> {
        let result = process::run_command(
            [
                "helm",
                "status",
                release_name(target),
                "--namespace",
                self.config.namespace(target),
                "-o",
                "json",
            ],
            None,
            None,
            60,
            false,
            None,
        )
        .await?;
        if result.status != 0 {
            return Ok(None);
        }
        Ok(Some(
            serde_json::from_str(&result.stdout).context("helm status returned invalid JSON")?,
        ))
    }

    pub async fn test_release(&self, target: &str) -> Result<()> {
        process::run_command(
            [
                "helm",
                "test",
                release_name(target),
                "--namespace",
                self.config.namespace(target),
                "--timeout",
                "5m",
            ],
            None,
            None,
            360,
            true,
            None,
        )
        .await?;
        Ok(())
    }
}

fn path(path: impl Into<PathBuf>) -> String {
    path.into().to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::{self, test_support};
    use std::sync::Arc;

    fn config() -> DeploymentConfig {
        DeploymentConfig::load(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../deploy/values/local.yaml"),
            None,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn tool_versions_accepts_helm_three_and_reads_kubectl_json() {
        let executor = Arc::new(test_support::RecordingExecutor::new(|request| {
            let stdout = match request.command.as_slice() {
                [program, command, ..] if program == "helm" && command == "version" => {
                    "v3.18.4+gd80839c"
                }
                [program, command, ..] if program == "kubectl" && command == "version" => {
                    r#"{"clientVersion":{"gitVersion":"v1.33.4"}}"#
                }
                _ => "",
            };
            Ok(test_support::result(request, 0, stdout))
        }));
        let assets = EmbeddedAssets::extract().unwrap();
        let config = config();
        let versions = process::with_command_executor(
            executor,
            Helm {
                config: &config,
                assets: &assets,
            }
            .tool_versions(),
        )
        .await
        .unwrap();
        assert_eq!(versions["helm"], "v3.18.4+gd80839c");
        assert_eq!(versions["kubectl"], "v1.33.4");
    }

    #[tokio::test]
    async fn tool_versions_rejects_helm_two() {
        let executor = Arc::new(test_support::RecordingExecutor::new(|request| {
            let stdout = if request.command.first().map(String::as_str) == Some("helm") {
                "v2.17.0"
            } else {
                r#"{"clientVersion":{"gitVersion":"v1.33.4"}}"#
            };
            Ok(test_support::result(request, 0, stdout))
        }));
        let assets = EmbeddedAssets::extract().unwrap();
        let config = config();
        let error = process::with_command_executor(
            executor,
            Helm {
                config: &config,
                assets: &assets,
            }
            .tool_versions(),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(error.contains("Helm 3 is required"));
    }

    #[tokio::test]
    async fn ingress_install_uses_only_the_embedded_chart() {
        let executor = Arc::new(test_support::RecordingExecutor::new(test_support::success));
        let assets = EmbeddedAssets::extract().unwrap();
        let config = config();
        process::with_command_executor(
            executor.clone(),
            Helm {
                config: &config,
                assets: &assets,
            }
            .install_ingress(),
        )
        .await
        .unwrap();
        let requests = executor.requests();
        assert_eq!(requests.len(), 1);
        let command = &requests[0].command;
        assert_eq!(&command[..3], ["helm", "upgrade", "--install"]);
        assert!(
            command
                .iter()
                .any(|argument| argument.ends_with("ingress-nginx-4.15.1.tgz"))
        );
        assert!(!command.iter().any(|argument| argument.starts_with("http://") || argument.starts_with("https://")));
        assert!(!command.iter().any(|argument| argument == "repo"));
    }

    #[tokio::test]
    async fn release_test_does_not_request_helm_hook_logs() {
        let executor = Arc::new(test_support::RecordingExecutor::new(test_support::success));
        let assets = EmbeddedAssets::extract().unwrap();
        let config = config();
        process::with_command_executor(
            executor.clone(),
            Helm {
                config: &config,
                assets: &assets,
            }
            .test_release("dependencies"),
        )
        .await
        .unwrap();

        let requests = executor.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].command,
            [
                "helm",
                "test",
                "agentx-dependencies",
                "--namespace",
                "agentx-deps",
                "--timeout",
                "5m",
            ]
        );
    }
}
