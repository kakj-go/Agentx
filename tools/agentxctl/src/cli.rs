use crate::{
    assets::EmbeddedAssets,
    config::{DeploymentConfig, selected_targets},
    operations,
    output::{OutputFormat, emit},
    process::redact,
};
use anyhow::{Result, anyhow};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde_json::{Value, json};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "agentxctl",
    version,
    about = "Agentx cluster deployment and operations CLI"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Args, Clone)]
struct Common {
    #[arg(
        long,
        help = "Values YAML; defaults to the embedded Docker Hub Beta configuration"
    )]
    values: Option<PathBuf>,
    #[arg(long, value_enum, default_value_t = Target::All)]
    target: Target,
    #[arg(long, help = "E2E-only namespace suffix")]
    run_id: Option<String>,
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    output: OutputFormat,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Target {
    Control,
    Runtime,
    Observability,
    Dependencies,
    All,
}

impl Target {
    fn name(self) -> &'static str {
        match self {
            Self::Control => "control",
            Self::Runtime => "runtime",
            Self::Observability => "observability",
            Self::Dependencies => "dependencies",
            Self::All => "all",
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum MigrationPhase {
    Expand,
    Contract,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum RotationAction {
    Plan,
    Rotate,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum DataTarget {
    ControlMysql,
    RuntimeMysql,
    ControlObjects,
    RuntimeObjects,
    ObservabilityObjects,
    Clickhouse,
}

impl DataTarget {
    fn name(self) -> &'static str {
        match self {
            Self::ControlMysql => "control-mysql",
            Self::RuntimeMysql => "runtime-mysql",
            Self::ControlObjects => "control-objects",
            Self::RuntimeObjects => "runtime-objects",
            Self::ObservabilityObjects => "observability-objects",
            Self::Clickhouse => "clickhouse",
        }
    }
}

#[derive(Subcommand)]
enum Command {
    Validate {
        #[command(flatten)]
        common: Common,
        #[arg(long)]
        cluster: bool,
    },
    Render {
        #[command(flatten)]
        common: Common,
    },
    Install {
        #[command(flatten)]
        common: Common,
        #[arg(long)]
        skip_doctor: bool,
    },
    Upgrade {
        #[command(flatten)]
        common: Common,
        #[arg(long)]
        skip_doctor: bool,
    },
    Status {
        #[command(flatten)]
        common: Common,
    },
    Doctor {
        #[command(flatten)]
        common: Common,
    },
    Rollback {
        #[command(flatten)]
        common: Common,
        #[arg(long)]
        revision: u64,
    },
    Uninstall {
        #[command(flatten)]
        common: Common,
        #[arg(long)]
        purge_data: bool,
        #[arg(long)]
        yes: bool,
    },
    Migrate {
        #[command(flatten)]
        common: Common,
        #[arg(long, value_enum, default_value_t = MigrationPhase::Expand)]
        phase: MigrationPhase,
        #[arg(long, default_value_t = 600)]
        timeout: u64,
    },
    SyncSecrets {
        #[command(flatten)]
        common: Common,
    },
    RotateEgressKeys {
        #[arg(long)]
        values: PathBuf,
        #[arg(long)]
        run_id: Option<String>,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
        #[arg(long, value_enum, default_value_t = RotationAction::Plan)]
        action: RotationAction,
    },
    Backup {
        #[arg(long)]
        values: PathBuf,
        #[arg(long)]
        run_id: Option<String>,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
        #[arg(long, value_enum)]
        data_target: DataTarget,
        #[arg(long)]
        backup_id: String,
        #[arg(long)]
        adapter: PathBuf,
        #[arg(long, default_value = "artifacts/data-operations")]
        artifact_dir: PathBuf,
    },
    Restore {
        #[arg(long)]
        values: PathBuf,
        #[arg(long)]
        run_id: Option<String>,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
        #[arg(long, value_enum)]
        data_target: DataTarget,
        #[arg(long)]
        backup_id: String,
        #[arg(long)]
        adapter: PathBuf,
        #[arg(long)]
        restore_target: String,
        #[arg(long)]
        allow_in_place_restore: bool,
        #[arg(long, default_value = "artifacts/data-operations")]
        artifact_dir: PathBuf,
    },
}

impl Command {
    fn output(&self) -> OutputFormat {
        match self {
            Self::Validate { common, .. }
            | Self::Render { common }
            | Self::Install { common, .. }
            | Self::Upgrade { common, .. }
            | Self::Status { common }
            | Self::Doctor { common }
            | Self::Rollback { common, .. }
            | Self::Uninstall { common, .. }
            | Self::Migrate { common, .. }
            | Self::SyncSecrets { common } => common.output,
            Self::RotateEgressKeys { output, .. }
            | Self::Backup { output, .. }
            | Self::Restore { output, .. } => *output,
        }
    }
}

pub async fn run() -> Result<()> {
    let cli = Cli::parse();
    let output = cli.command.output();
    match execute(cli.command).await {
        Ok(value) => emit(&value, output),
        Err(error) => {
            let message = redact(&format!("{error:#}"));
            match output {
                OutputFormat::Json => eprintln!(
                    "{}",
                    serde_json::to_string(&json!({"status":"error","error":message}))?
                ),
                OutputFormat::Text => eprintln!("agentxctl: {message}"),
            }
            std::process::exit(1);
        }
    }
}

async fn execute(command: Command) -> Result<Value> {
    let assets = EmbeddedAssets::extract()?;
    match command {
        Command::Validate { common, cluster } => {
            let (config, targets) = load_common(&common)?;
            operations::validate(&config, &assets, &targets, cluster).await
        }
        Command::Render { common } => {
            let (config, targets) = load_common(&common)?;
            let manifest = operations::render(&config, &assets, &targets).await?;
            Ok(match common.output {
                OutputFormat::Text => Value::String(manifest),
                OutputFormat::Json => json!({"status":"rendered","manifest":manifest}),
            })
        }
        Command::Install {
            common,
            skip_doctor,
        } => {
            let (config, targets) = load_common(&common)?;
            operations::install(&config, &assets, &targets, !skip_doctor, false).await
        }
        Command::Upgrade {
            common,
            skip_doctor,
        } => {
            let (config, targets) = load_common(&common)?;
            operations::install(&config, &assets, &targets, !skip_doctor, true).await
        }
        Command::Status { common } => {
            let (config, targets) = load_common(&common)?;
            operations::status(&config, &assets, &targets, None).await
        }
        Command::Doctor { common } => {
            let (config, targets) = load_common(&common)?;
            operations::doctor(&config, &assets, &targets).await
        }
        Command::Rollback { common, revision } => {
            let (config, _) = load_common(&common)?;
            operations::rollback(&config, &assets, common.target.name(), revision).await
        }
        Command::Uninstall {
            common,
            purge_data,
            yes,
        } => {
            let (config, targets) = load_common(&common)?;
            operations::uninstall(&config, &assets, &targets, purge_data, yes).await
        }
        Command::Migrate {
            common,
            phase,
            timeout,
        } => {
            let (config, _) = load_common(&common)?;
            if matches!(common.target, Target::All) {
                return Err(anyhow!("migrate requires one explicit target"));
            }
            operations::migrate(
                &config,
                &assets,
                common.target.name(),
                matches!(phase, MigrationPhase::Contract),
                timeout,
            )
            .await
        }
        Command::SyncSecrets { common } => {
            if !matches!(common.target, Target::All) {
                return Err(anyhow!("sync-secrets requires --target all"));
            }
            let config = load_config(common.values.as_ref(), common.run_id.as_deref())?;
            operations::sync_secrets(&config, &assets).await
        }
        Command::RotateEgressKeys {
            values,
            run_id,
            action,
            ..
        } => {
            let config = DeploymentConfig::load(values, run_id.as_deref())?;
            crate::secrets::rotate_egress_keys(&config, matches!(action, RotationAction::Rotate))
                .await
        }
        Command::Backup {
            values,
            run_id,
            data_target,
            backup_id,
            adapter,
            artifact_dir,
            ..
        } => {
            let config = DeploymentConfig::load(values, run_id.as_deref())?;
            crate::backup::operate(
                &config,
                "backup",
                data_target.name(),
                &backup_id,
                &adapter,
                None,
                &artifact_dir,
                false,
            )
            .await
        }
        Command::Restore {
            values,
            run_id,
            data_target,
            backup_id,
            adapter,
            restore_target,
            allow_in_place_restore,
            artifact_dir,
            ..
        } => {
            let config = DeploymentConfig::load(values, run_id.as_deref())?;
            crate::backup::operate(
                &config,
                "restore",
                data_target.name(),
                &backup_id,
                &adapter,
                Some(&restore_target),
                &artifact_dir,
                allow_in_place_restore,
            )
            .await
        }
    }
}

fn load_common(common: &Common) -> Result<(DeploymentConfig, Vec<&'static str>)> {
    Ok((
        load_config(common.values.as_ref(), common.run_id.as_deref())?,
        selected_targets(common.target.name())?,
    ))
}

fn load_config(values: Option<&PathBuf>, run_id: Option<&str>) -> Result<DeploymentConfig> {
    match values {
        Some(path) => DeploymentConfig::load(path, run_id),
        None => DeploymentConfig::load_embedded_beta(run_id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_public_subcommand_has_a_parseable_contract() {
        let cases = [
            vec!["validate"],
            vec!["render", "--target", "runtime"],
            vec!["install"],
            vec!["status", "--output", "json"],
            vec!["doctor"],
            vec!["uninstall", "--purge-data", "--yes"],
            vec!["validate", "--values", "values.yaml"],
            vec!["render", "--values", "values.yaml", "--target", "runtime"],
            vec!["install", "--values", "values.yaml", "--skip-doctor"],
            vec!["upgrade", "--values", "values.yaml"],
            vec!["status", "--values", "values.yaml", "--output", "json"],
            vec!["doctor", "--values", "values.yaml"],
            vec![
                "rollback",
                "--values",
                "values.yaml",
                "--target",
                "runtime",
                "--revision",
                "2",
            ],
            vec![
                "uninstall",
                "--values",
                "values.yaml",
                "--purge-data",
                "--yes",
            ],
            vec![
                "migrate",
                "--values",
                "values.yaml",
                "--target",
                "control",
                "--phase",
                "contract",
            ],
            vec!["sync-secrets", "--values", "values.yaml"],
            vec![
                "rotate-egress-keys",
                "--values",
                "values.yaml",
                "--action",
                "rotate",
            ],
            vec![
                "backup",
                "--values",
                "values.yaml",
                "--data-target",
                "control-mysql",
                "--backup-id",
                "backup-1",
                "--adapter",
                "adapter",
            ],
            vec![
                "restore",
                "--values",
                "values.yaml",
                "--data-target",
                "runtime-mysql",
                "--backup-id",
                "backup-1",
                "--adapter",
                "adapter",
                "--restore-target",
                "restored.example.internal",
            ],
        ];
        for arguments in cases {
            let mut command = vec!["agentxctl"];
            command.extend(arguments);
            Cli::try_parse_from(command).unwrap();
        }
        assert!(Cli::try_parse_from(["agentxctl", "build-images"]).is_err());
    }
}
