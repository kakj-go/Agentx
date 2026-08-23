use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use regex::Regex;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    future::Future,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};
use tokio::{io::AsyncWriteExt, process::Command, time::timeout};

#[derive(Debug, Clone)]
pub struct CommandResult {
    pub command: Vec<String>,
    pub stdout: String,
    pub stderr: String,
    pub status: i32,
}

impl CommandResult {
    pub fn json(&self) -> Result<Value> {
        serde_json::from_str(&self.stdout).context("command returned invalid JSON")
    }
}

#[derive(Debug, Clone)]
pub struct CommandRequest {
    pub command: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub input: Option<String>,
    pub timeout_seconds: u64,
    pub env: Option<BTreeMap<String, String>>,
}

#[async_trait]
pub trait CommandExecutor: Send + Sync {
    async fn execute(&self, request: CommandRequest) -> Result<CommandResult>;
}

#[derive(Debug, Default)]
pub struct SystemCommandExecutor;

tokio::task_local! {
    static COMMAND_EXECUTOR: Arc<dyn CommandExecutor>;
}

pub async fn with_command_executor<T, F>(executor: Arc<dyn CommandExecutor>, future: F) -> T
where
    F: Future<Output = T>,
{
    COMMAND_EXECUTOR.scope(executor, future).await
}

pub fn redact(value: &str) -> String {
    let value = redact_embedded_material(value);
    let sensitive = Regex::new(
        r#"(?i)((?:password|secret|token|private[_-]?key|authorization)["']?\s*[=:]\s*["']?)([^"'\s,;}]+)"#,
    )
    .unwrap();
    sensitive.replace_all(&value, "$1<redacted>").into_owned()
}

pub fn redact_embedded_material(value: &str) -> String {
    let url = Regex::new(r"(?i)([a-z][a-z0-9+.-]*://)[^/@\s]+:[^/@\s]+@").unwrap();
    let pem = Regex::new(
        r"(?s)-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----.*?-----END (?:RSA |EC |OPENSSH )?PRIVATE KEY-----",
    )
    .unwrap();
    let value = url.replace_all(value, "$1<redacted>@");
    pem.replace_all(&value, "<redacted-private-key>")
        .into_owned()
}

pub fn find_tool(name: &str) -> Result<PathBuf> {
    if COMMAND_EXECUTOR.try_with(|_| ()).is_ok() {
        return Ok(PathBuf::from(name));
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    let extensions: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
            .split(';')
            .map(str::to_owned)
            .collect()
    } else {
        vec![String::new()]
    };
    for directory in std::env::split_paths(&path) {
        for extension in &extensions {
            let candidate = directory.join(format!("{name}{extension}"));
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    bail!("required tool is not installed or not on PATH: {name}")
}

pub async fn run_command<I, S>(
    command: I,
    cwd: Option<&Path>,
    input: Option<&str>,
    timeout_seconds: u64,
    check: bool,
    env: Option<&BTreeMap<String, String>>,
) -> Result<CommandResult>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let args: Vec<String> = command.into_iter().map(Into::into).collect();
    if args.is_empty() {
        bail!("empty command");
    }
    let request = CommandRequest {
        command: args,
        cwd: cwd.map(Path::to_path_buf),
        input: input.map(str::to_owned),
        timeout_seconds,
        env: env.cloned(),
    };
    let executor = COMMAND_EXECUTOR
        .try_with(Arc::clone)
        .unwrap_or_else(|_| Arc::new(SystemCommandExecutor));
    let result = executor.execute(request).await?;
    if check && result.status != 0 {
        bail!(
            "command failed ({}): {}\n{}",
            result.status,
            redact(&result.command.join(" ")),
            redact(&format!("{}{}", result.stdout, result.stderr)).trim()
        );
    }
    Ok(result)
}

#[async_trait]
impl CommandExecutor for SystemCommandExecutor {
    async fn execute(&self, request: CommandRequest) -> Result<CommandResult> {
        let (program, program_args) = request
            .command
            .split_first()
            .ok_or_else(|| anyhow!("empty command"))?;
        let mut child = Command::new(program);
        child
            .args(program_args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if request.input.is_some() {
            child.stdin(Stdio::piped());
        } else {
            child.stdin(Stdio::null());
        }
        if let Some(cwd) = &request.cwd {
            child.current_dir(cwd);
        }
        if let Some(env) = &request.env {
            child.envs(env);
        }
        let mut child = child
            .spawn()
            .with_context(|| format!("start command: {}", redact(program)))?;
        if let Some(input) = &request.input {
            child
                .stdin
                .take()
                .context("command stdin is unavailable")?
                .write_all(input.as_bytes())
                .await?;
        }
        let output = timeout(
            Duration::from_secs(request.timeout_seconds),
            child.wait_with_output(),
        )
        .await
        .map_err(|_| {
            anyhow!(
                "command timed out after {}s: {}",
                request.timeout_seconds,
                redact(&request.command.join(" "))
            )
        })??;
        let result = CommandResult {
            command: request.command,
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            status: output.status.code().unwrap_or(-1),
        };
        Ok(result)
    }
}

#[cfg(test)]
pub mod test_support {
    use super::*;
    use std::sync::Mutex;

    type Handler = dyn Fn(&CommandRequest) -> Result<CommandResult> + Send + Sync;

    pub struct RecordingExecutor {
        requests: Mutex<Vec<CommandRequest>>,
        handler: Box<Handler>,
    }

    impl RecordingExecutor {
        pub fn new(
            handler: impl Fn(&CommandRequest) -> Result<CommandResult> + Send + Sync + 'static,
        ) -> Self {
            Self {
                requests: Mutex::new(Vec::new()),
                handler: Box::new(handler),
            }
        }

        pub fn requests(&self) -> Vec<CommandRequest> {
            self.requests.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl CommandExecutor for RecordingExecutor {
        async fn execute(&self, request: CommandRequest) -> Result<CommandResult> {
            self.requests.lock().unwrap().push(request.clone());
            (self.handler)(&request)
        }
    }

    pub fn result(
        request: &CommandRequest,
        status: i32,
        stdout: impl Into<String>,
    ) -> CommandResult {
        CommandResult {
            command: request.command.clone(),
            stdout: stdout.into(),
            stderr: String::new(),
            status,
        }
    }

    pub fn success(request: &CommandRequest) -> Result<CommandResult> {
        Ok(result(request, 0, ""))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_credentials_and_private_material() {
        let input = "password=hunter2 https://user:pass@example.test -----BEGIN PRIVATE KEY-----\nabc\n-----END PRIVATE KEY-----";
        let output = redact(input);
        assert!(!output.contains("hunter2"));
        assert!(!output.contains("user:pass"));
        assert!(!output.contains("\nabc\n"));
    }

    #[tokio::test]
    async fn system_executor_enforces_timeout() {
        let command = if cfg!(windows) {
            vec![
                "powershell".to_owned(),
                "-NoProfile".to_owned(),
                "-Command".to_owned(),
                "Start-Sleep -Seconds 2".to_owned(),
            ]
        } else {
            vec!["sh".to_owned(), "-c".to_owned(), "sleep 2".to_owned()]
        };
        let error = SystemCommandExecutor
            .execute(CommandRequest {
                command,
                cwd: None,
                input: None,
                timeout_seconds: 0,
                env: None,
            })
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("timed out after 0s"));
    }

    #[tokio::test]
    async fn injected_executor_preserves_check_and_redaction_semantics() {
        let executor = Arc::new(test_support::RecordingExecutor::new(|request| {
            Ok(test_support::result(request, 7, "password=hunter2"))
        }));
        let unchecked = with_command_executor(
            executor.clone(),
            run_command(["fixture"], None, None, 1, false, None),
        )
        .await
        .unwrap();
        assert_eq!(unchecked.status, 7);
        let error = with_command_executor(
            executor,
            run_command(["fixture"], None, None, 1, true, None),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(error.contains("command failed (7)"));
        assert!(!error.contains("hunter2"));
    }
}
