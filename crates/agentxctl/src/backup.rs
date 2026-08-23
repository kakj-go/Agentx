use crate::{assets::BACKUP_SCHEMA, config::DeploymentConfig, process};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use std::path::Path;

#[allow(clippy::too_many_arguments)]
pub async fn operate(
    config: &DeploymentConfig,
    action: &str,
    target: &str,
    backup_id: &str,
    adapter: &Path,
    restore_target: Option<&str>,
    artifact_dir: &Path,
    allow_in_place_restore: bool,
) -> Result<Value> {
    if config.environment() != "production" {
        bail!("backup and restore acceptance requires production values");
    }
    let adapter = adapter
        .canonicalize()
        .with_context(|| format!("backup adapter does not exist: {}", adapter.display()))?;
    if !adapter.is_file() {
        bail!("backup adapter is not a file: {}", adapter.display());
    }
    ensure_executable(&adapter)?;
    if action == "restore" && restore_target.is_none() {
        bail!("restore requires --restore-target");
    }
    let authoritative = match target {
        "control-mysql" => config.string("/global/components/controlMysql/host"),
        "runtime-mysql" => config.string("/global/components/runtimeMysql/host"),
        "clickhouse" => config.string("/global/components/clickhouse/url"),
        _ => None,
    };
    if action == "restore" && authoritative == restore_target && !allow_in_place_restore {
        bail!("in-place restore requires --allow-in-place-restore");
    }
    let started = Utc::now();
    let mut args = vec![
        adapter.to_string_lossy().into_owned(),
        "--action".into(),
        action.into(),
        "--target".into(),
        target.into(),
        "--backup-id".into(),
        backup_id.into(),
        "--values".into(),
        config.path.to_string_lossy().into_owned(),
    ];
    if let Some(restore_target) = restore_target {
        args.extend(["--restore-target".into(), restore_target.into()]);
    }
    let receipt = process::run_command(args, None, None, 3600, true, None)
        .await?
        .json()?;
    if receipt.get("status").and_then(Value::as_str) != Some("passed") {
        bail!("backup provider adapter did not return status=passed");
    }
    let expected = [
        "status",
        "recoveryPointUtc",
        "objectCount",
        "contentSha256",
        "schemaVersionObserved",
    ];
    let object = receipt
        .as_object()
        .context("backup provider receipt must be a JSON object")?;
    if object.len() != expected.len() || expected.iter().any(|key| !object.contains_key(*key)) {
        bail!("backup provider receipt has missing or unapproved fields");
    }
    let completed = Utc::now();
    let recovery_point = DateTime::parse_from_rfc3339(
        receipt
            .get("recoveryPointUtc")
            .and_then(Value::as_str)
            .context("recoveryPointUtc must be a string")?,
    )
    .context("provider recoveryPointUtc must include a timezone")?
    .with_timezone(&Utc);
    let age_minutes = (completed - recovery_point).num_milliseconds() as f64 / 60_000.0;
    if age_minutes < -1.0 {
        bail!("provider recovery point is unexpectedly in the future");
    }
    let prefix = if target.ends_with("-mysql") {
        "mysql"
    } else if target.ends_with("-objects") {
        "object"
    } else {
        "clickhouse"
    };
    let rpo = config
        .u64(&format!("/global/backup/{prefix}RpoMinutes"))
        .context("missing backup RPO setting")? as f64;
    let rto = config
        .u64(&format!("/global/backup/{prefix}RtoMinutes"))
        .context("missing backup RTO setting")? as f64;
    let elapsed_minutes = (completed - started).num_milliseconds() as f64 / 60_000.0;
    if action == "backup" && age_minutes > rpo {
        bail!("RPO exceeded: {age_minutes:.2}m > {rpo}m");
    }
    if action == "restore" && elapsed_minutes > rto {
        bail!("RTO exceeded: {elapsed_minutes:.2}m > {rto}m");
    }
    let mut evidence = json!({
        "schemaVersion": "agentx.io/backup-manifest/v1",
        "backupId": backup_id,
        "target": target,
        "operation": action,
        "status": "passed",
        "startedAt": started.to_rfc3339(),
        "completedAt": completed.to_rfc3339(),
        "recoveryPointUtc": recovery_point.to_rfc3339(),
        "objectCount": receipt.get("objectCount").and_then(Value::as_u64).context("objectCount must be a non-negative integer")?,
        "contentSha256": receipt.get("contentSha256").and_then(Value::as_str).context("contentSha256 must be a string")?,
        "schemaVersionObserved": receipt.get("schemaVersionObserved").and_then(Value::as_str).context("schemaVersionObserved must be a string")?,
        "restoreTarget": restore_target,
        "providerReceipt": receipt,
    });
    let schema: Value = serde_json::from_str(BACKUP_SCHEMA)?;
    jsonschema::validator_for(&schema)?
        .validate(&evidence)
        .map_err(|error| anyhow::anyhow!("backup evidence validation failed: {error}"))?;
    let artifact_dir = if artifact_dir.is_absolute() {
        artifact_dir.to_path_buf()
    } else {
        std::env::current_dir()?.join(artifact_dir)
    };
    std::fs::create_dir_all(&artifact_dir)?;
    let output = artifact_dir.join(format!("{backup_id}-{target}-{action}.json"));
    std::fs::write(&output, serde_json::to_vec_pretty(&evidence)?)?;
    evidence
        .as_object_mut()
        .unwrap()
        .insert("path".into(), output.to_string_lossy().into_owned().into());
    Ok(evidence)
}

fn ensure_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::metadata(path)?.permissions().mode() & 0o111 == 0 {
            bail!("backup adapter is not executable: {}", path.display());
        }
    }
    #[cfg(windows)]
    {
        let executable = path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                ["exe", "com", "bat", "cmd"]
                    .iter()
                    .any(|candidate| extension.eq_ignore_ascii_case(candidate))
            });
        if !executable {
            bail!("backup adapter is not executable: {}", path.display());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::{self, CommandRequest, test_support};
    use std::{path::PathBuf, sync::Arc, time::Duration};

    fn production_config() -> DeploymentConfig {
        DeploymentConfig::load(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../deploy/values/production.example.yaml"),
            None,
        )
        .unwrap()
    }

    fn executable(directory: &Path) -> std::path::PathBuf {
        let extension = if cfg!(windows) { "cmd" } else { "sh" };
        let path = directory.join(format!("adapter.{extension}"));
        std::fs::write(
            &path,
            if cfg!(windows) {
                "@exit /b 0"
            } else {
                "#!/bin/sh\nexit 0\n"
            },
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(&path).unwrap().permissions();
            permissions.set_mode(0o700);
            std::fs::set_permissions(&path, permissions).unwrap();
        }
        path
    }

    fn receipt(recovery_point: &str) -> String {
        json!({
            "status":"passed",
            "recoveryPointUtc":recovery_point,
            "objectCount":7,
            "contentSha256":"0".repeat(64),
            "schemaVersionObserved":"control-0006"
        })
        .to_string()
    }

    fn receipt_executor(payload: String) -> Arc<test_support::RecordingExecutor> {
        Arc::new(test_support::RecordingExecutor::new(move |request| {
            Ok(test_support::result(request, 0, payload.clone()))
        }))
    }

    #[test]
    fn adapter_must_be_an_executable_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("adapter.py");
        std::fs::write(&path, "print('receipt')").unwrap();
        let error = ensure_executable(&path).unwrap_err().to_string();
        assert!(error.contains("not executable"));
    }

    #[tokio::test]
    async fn valid_receipt_writes_schema_valid_evidence() {
        let directory = tempfile::tempdir().unwrap();
        let adapter = executable(directory.path());
        let artifacts = directory.path().join("artifacts");
        let executor = receipt_executor(receipt(&Utc::now().to_rfc3339()));
        let config = production_config();
        let evidence = process::with_command_executor(
            executor.clone(),
            operate(
                &config,
                "backup",
                "control-mysql",
                "unit-backup",
                &adapter,
                None,
                &artifacts,
                false,
            ),
        )
        .await
        .unwrap();
        assert_eq!(evidence["status"], "passed");
        assert_eq!(evidence["objectCount"], 7);
        assert!(Path::new(evidence["path"].as_str().unwrap()).is_file());
        let command = &executor.requests()[0].command;
        assert_eq!(
            command[1..5],
            ["--action", "backup", "--target", "control-mysql"]
        );
    }

    #[tokio::test]
    async fn receipt_field_whitelist_is_exact() {
        let directory = tempfile::tempdir().unwrap();
        let adapter = executable(directory.path());
        let mut payload: Value = serde_json::from_str(&receipt(&Utc::now().to_rfc3339())).unwrap();
        payload["unexpected"] = true.into();
        let config = production_config();
        let error = process::with_command_executor(
            receipt_executor(payload.to_string()),
            operate(
                &config,
                "backup",
                "control-mysql",
                "unit-fields",
                &adapter,
                None,
                directory.path(),
                false,
            ),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(error.contains("missing or unapproved fields"));
    }

    #[tokio::test]
    async fn recovery_point_requires_timezone_and_cannot_be_future() {
        let directory = tempfile::tempdir().unwrap();
        let adapter = executable(directory.path());
        let config = production_config();
        for (point, expected) in [
            ("2026-01-01T00:00:00", "include a timezone"),
            ("2999-01-01T00:00:00Z", "future"),
        ] {
            let error = process::with_command_executor(
                receipt_executor(receipt(point)),
                operate(
                    &config,
                    "backup",
                    "control-mysql",
                    "unit-time",
                    &adapter,
                    None,
                    directory.path(),
                    false,
                ),
            )
            .await
            .unwrap_err()
            .to_string();
            assert!(error.contains(expected), "{error}");
        }
    }

    #[tokio::test]
    async fn rpo_rto_and_in_place_restore_are_enforced() {
        let directory = tempfile::tempdir().unwrap();
        let adapter = executable(directory.path());
        let mut config = production_config();
        let old = receipt("2020-01-01T00:00:00Z");
        let rpo_error = process::with_command_executor(
            receipt_executor(old),
            operate(
                &config,
                "backup",
                "control-mysql",
                "unit-rpo",
                &adapter,
                None,
                directory.path(),
                false,
            ),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(rpo_error.contains("RPO exceeded"));

        let authoritative = config
            .string("/global/components/controlMysql/host")
            .unwrap()
            .to_owned();
        let in_place = operate(
            &config,
            "restore",
            "control-mysql",
            "unit-in-place",
            &adapter,
            Some(&authoritative),
            directory.path(),
            false,
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(in_place.contains("allow-in-place-restore"));

        *config
            .values
            .pointer_mut("/global/backup/mysqlRtoMinutes")
            .unwrap() = 0.into();
        let payload = receipt(&Utc::now().to_rfc3339());
        let executor = Arc::new(test_support::RecordingExecutor::new(
            move |request: &CommandRequest| {
                std::thread::sleep(Duration::from_millis(10));
                Ok(test_support::result(request, 0, payload.clone()))
            },
        ));
        let rto_error = process::with_command_executor(
            executor,
            operate(
                &config,
                "restore",
                "control-mysql",
                "unit-rto",
                &adapter,
                Some("restored-control.example.internal"),
                directory.path(),
                false,
            ),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(rto_error.contains("RTO exceeded"));
    }
}
