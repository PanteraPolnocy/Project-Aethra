//! Owns the connection to the inference server and, when configured, the
//! server process itself. Profiles differ per mode so overnight learning can
//! stay on the CPU while chat may borrow the GPU.

use std::sync::Arc;
use std::time::{Duration, Instant};

use aethra_models::openai_compat::{OpenAiCompatConfig, OpenAiCompatModel};
use aethra_models::sidecar::{LlamaServerProfile, Sidecar, SidecarSpec};
use aethra_models::{LanguageModel, ModelError};
use parking_lot::Mutex;

use crate::config::{AppConfig, ModelProfile};
use crate::error::{CoreError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Chat,
    Learning,
}

impl Profile {
    pub fn as_str(&self) -> &'static str {
        match self {
            Profile::Chat => "chat",
            Profile::Learning => "learning",
        }
    }
}

struct Running {
    profile: Profile,
    launch: LlamaServerProfile,
    sidecar: Sidecar,
}

pub struct ModelHost {
    cfg: Arc<AppConfig>,
    model: Arc<OpenAiCompatModel>,
    running: tokio::sync::Mutex<Option<Running>>,
    last_used: Mutex<Instant>,
}

impl ModelHost {
    pub fn new(cfg: Arc<AppConfig>) -> Result<Self> {
        let model = OpenAiCompatModel::new(OpenAiCompatConfig {
            id: if cfg.models.sidecar.enabled {
                "local:llama-server".to_string()
            } else {
                format!("endpoint:{}", cfg.models.endpoint)
            },
            base_url: cfg.effective_endpoint(),
            model_name: cfg.models.model_name.clone(),
            api_key: cfg.models.api_key.clone(),
            request_timeout: Duration::from_secs(cfg.models.request_timeout_secs.max(30)),
        })?;
        Ok(Self {
            cfg,
            model: Arc::new(model),
            running: tokio::sync::Mutex::new(None),
            last_used: Mutex::new(Instant::now()),
        })
    }

    pub fn profile_config(&self, profile: Profile) -> &ModelProfile {
        match profile {
            Profile::Chat => &self.cfg.models.chat,
            Profile::Learning => &self.cfg.models.learning,
        }
    }

    fn launch_profile(&self, profile: Profile) -> Result<LlamaServerProfile> {
        let p = self.profile_config(profile);
        let model_path = p
            .model_path
            .clone()
            .or_else(|| self.cfg.models.chat.model_path.clone())
            .ok_or_else(|| CoreError::Config("no model_path configured".into()))?;
        Ok(LlamaServerProfile {
            model_path,
            port: self.cfg.models.sidecar.port,
            ctx_size: p.ctx_size,
            threads: p.threads,
            gpu_layers: p.gpu_layers,
            extra_args: p.extra_args.clone(),
        })
    }

    pub fn touch(&self) {
        *self.last_used.lock() = Instant::now();
    }

    pub fn idle_for(&self) -> Duration {
        self.last_used.lock().elapsed()
    }

    /// Makes sure a model answering `profile` is reachable and returns it.
    /// With the sidecar disabled this is just a health check.
    pub async fn ensure(&self, profile: Profile) -> Result<Arc<dyn LanguageModel>> {
        self.touch();
        if !self.cfg.models.sidecar.enabled {
            self.model.health().await?;
            return Ok(self.model.clone());
        }

        let wanted = self.launch_profile(profile)?;
        let mut guard = self.running.lock().await;
        if let Some(r) = guard.as_mut() {
            if r.launch == wanted && r.sidecar.is_running() {
                return Ok(self.model.clone());
            }
        }
        if let Some(old) = guard.take() {
            tracing::info!(from = old.profile.as_str(), to = profile.as_str(), "switching inference profile");
            old.sidecar.stop().await;
        }
        let exe = self
            .cfg
            .models
            .sidecar
            .executable
            .clone()
            .ok_or_else(|| CoreError::Config("models.sidecar.executable is unset".into()))?;
        let spec = SidecarSpec {
            executable: exe,
            args: wanted.to_args(),
            health_url: format!("{}/health", self.model.base_url()),
            startup_timeout: Duration::from_secs(self.cfg.models.sidecar.startup_timeout_secs.max(10)),
        };
        tracing::info!(profile = profile.as_str(), model = %wanted.model_path.display(), gpu_layers = wanted.gpu_layers, "starting inference server");
        let sidecar = Sidecar::spawn(spec).await?;
        *guard = Some(Running {
            profile,
            launch: wanted,
            sidecar,
        });
        self.touch();
        Ok(self.model.clone())
    }

    /// Stops the sidecar if it has been unused for longer than `idle_after`.
    pub async fn unload_if_idle(&self, idle_after: Duration) -> bool {
        if self.idle_for() < idle_after {
            return false;
        }
        let mut guard = self.running.lock().await;
        if let Some(r) = guard.take() {
            tracing::info!(profile = r.profile.as_str(), "unloading idle inference server");
            r.sidecar.stop().await;
            return true;
        }
        false
    }

    pub async fn loaded_profile(&self) -> Option<Profile> {
        let mut guard = self.running.lock().await;
        let alive = match guard.as_mut() {
            Some(r) => r.sidecar.is_running(),
            None => return None,
        };
        if alive {
            guard.as_ref().map(|r| r.profile)
        } else {
            // The process died underneath us; forget it so the next ensure() respawns.
            *guard = None;
            None
        }
    }

    pub async fn reachable(&self) -> bool {
        self.model.health().await.is_ok()
    }

    pub fn model(&self) -> Arc<dyn LanguageModel> {
        self.model.clone()
    }

    pub async fn shutdown(&self) {
        if let Some(r) = self.running.lock().await.take() {
            r.sidecar.stop().await;
        }
    }

    pub fn describe_error(e: &ModelError) -> String {
        match e {
            ModelError::Unreachable(_) => "inference server unreachable".to_string(),
            ModelError::Timeout => "inference server timed out".to_string(),
            other => other.to_string(),
        }
    }
}
