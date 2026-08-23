use serde_json::Value;
use std::process::Command;

#[test]
fn help_exposes_operations_and_not_developer_commands() {
    let output = Command::new(env!("CARGO_BIN_EXE_agentxctl"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    for command in [
        "install",
        "upgrade",
        "rollback",
        "backup",
        "rotate-egress-keys",
    ] {
        assert!(stdout.contains(command));
    }
    assert!(!stdout.contains("build-images"));
}

#[test]
fn json_failure_is_machine_readable_and_uses_new_name() {
    let output = Command::new(env!("CARGO_BIN_EXE_agentxctl"))
        .args([
            "validate",
            "--values",
            "does-not-exist.yaml",
            "--output",
            "json",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["status"], "error");
    assert!(
        error["error"]
            .as_str()
            .unwrap()
            .contains("values file does not exist")
    );
}
