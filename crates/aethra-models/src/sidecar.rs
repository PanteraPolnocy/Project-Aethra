//! Supervision of a local inference server process (llama.cpp `llama-server`).
//!
//! The mind owns the process lifetime: it starts the server with the profile
//! the current mode needs (CPU-only for overnight learning, optional partial
//! GPU offload for chat) and stops it when nothing has needed the model for a
//! while, so the machine is not held hostage by an idle 20 GB model.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

use crate::ModelError;

#[derive(Debug, Clone)]
pub struct SidecarSpec {
    pub executable: PathBuf,
    pub args: Vec<String>,
    /// URL polled until it returns 2xx. llama-server: `http://127.0.0.1:PORT/health`.
    pub health_url: String,
    pub startup_timeout: Duration,
}

/// Arguments for one llama-server launch profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlamaServerProfile {
    pub model_path: PathBuf,
    pub port: u16,
    pub ctx_size: u32,
    pub threads: u32,
    pub gpu_layers: u32,
    pub extra_args: Vec<String>,
}

impl LlamaServerProfile {
    pub fn to_args(&self) -> Vec<String> {
        let mut args = vec![
            "--model".to_string(),
            self.model_path.to_string_lossy().into_owned(),
            "--host".to_string(),
            "127.0.0.1".to_string(),
            "--port".to_string(),
            self.port.to_string(),
            "--ctx-size".to_string(),
            self.ctx_size.to_string(),
            "--threads".to_string(),
            self.threads.to_string(),
            "--n-gpu-layers".to_string(),
            self.gpu_layers.to_string(),
            "--jinja".to_string(),
        ];
        args.extend(self.extra_args.iter().cloned());
        args
    }
}

pub struct Sidecar {
    spec: SidecarSpec,
    child: Child,
}

impl Sidecar {
    pub async fn spawn(spec: SidecarSpec) -> Result<Self, ModelError> {
        if !spec.executable.is_file() {
            return Err(ModelError::Config(format!(
                "inference server executable not found: {}",
                spec.executable.display()
            )));
        }
        let mut cmd = Command::new(&spec.executable);
        cmd.args(&spec.args)
            .kill_on_drop(true)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = cmd.spawn().map_err(|e| {
            ModelError::Config(format!("failed to start {}: {e}", spec.executable.display()))
        })?;
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::debug!(target: "inference_server", "{line}");
                }
            });
        }
        let mut this = Self { spec, child };
        if let Err(e) = this.wait_ready().await {
            let _ = this.child.start_kill();
            return Err(e);
        }
        Ok(this)
    }

    async fn wait_ready(&mut self) -> Result<(), ModelError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .map_err(|e| ModelError::Config(e.to_string()))?;
        let deadline = Instant::now() + self.spec.startup_timeout;
        loop {
            if let Ok(Some(status)) = self.child.try_wait() {
                return Err(ModelError::Unreachable(format!(
                    "inference server exited during startup ({status})"
                )));
            }
            if let Ok(resp) = client.get(&self.spec.health_url).send().await {
                if resp.status().is_success() {
                    return Ok(());
                }
            }
            if Instant::now() >= deadline {
                return Err(ModelError::Timeout);
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    pub fn spec(&self) -> &SidecarSpec {
        &self.spec
    }

    pub async fn stop(mut self) {
        let _ = self.child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(10), self.child.wait()).await;
    }
}
