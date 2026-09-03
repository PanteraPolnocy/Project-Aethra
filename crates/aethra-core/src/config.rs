//! Application configuration. Loaded once at startup from `config.toml`.
//!
//! Deliberately not writable by the agent: there is no tool that touches this
//! file, and the daemon reads it exactly once. Boundaries (network policy,
//! budgets, model launch profiles) live here and are Tier C: the mind may ask
//! for a change, the user edits the file.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

pub const APP_DIR_NAME: &str = "Aethra";
pub const CONFIG_FILE_NAME: &str = "config.toml";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// Where the SQLite files, logs and the agent workspace live.
    pub data_dir: PathBuf,
    pub identity: IdentityConfig,
    pub models: ModelsConfig,
    pub modes: ModesConfig,
    pub budgets: BudgetsConfig,
    pub network: NetworkConfig,
    pub context: ContextConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct IdentityConfig {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelsConfig {
    /// OpenAI-compatible endpoint. When the sidecar is enabled this is derived
    /// from `sidecar.port` and this value is ignored.
    pub endpoint: String,
    /// Model name sent in requests. llama-server ignores it; Ollama and remote
    /// providers need it.
    pub model_name: String,
    /// Bearer token for remote providers. Empty means none.
    #[serde(with = "empty_is_none")]
    pub api_key: Option<String>,
    pub request_timeout_secs: u64,
    pub sidecar: SidecarConfig,
    /// Launch profile used while chatting.
    pub chat: ModelProfile,
    /// Launch profile used for overnight learning. CPU-only by default.
    pub learning: ModelProfile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SidecarConfig {
    /// When true, Aethra starts and stops `executable` itself.
    /// When false, it expects a server already listening on `models.endpoint`.
    pub enabled: bool,
    /// Path to llama-server (or a compatible binary taking the same flags).
    /// Written as "" when unset so the key is visible in a fresh config.
    #[serde(with = "empty_is_none")]
    pub executable: Option<PathBuf>,
    pub port: u16,
    /// Cold-loading a 20 GB model from disk can take minutes.
    pub startup_timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelProfile {
    /// GGUF path. If "" for `learning`, the chat profile's model is reused.
    #[serde(with = "empty_is_none")]
    pub model_path: Option<PathBuf>,
    pub ctx_size: u32,
    pub threads: u32,
    /// 0 keeps everything on the CPU.
    pub gpu_layers: u32,
    pub extra_args: Vec<String>,
    /// f64 so TOML prints 0.7, not the widened f32 0.699999988079071.
    pub temperature: f64,
    pub max_tokens: u32,
}

/// Serialises `None` as an empty string and reads an empty or blank string
/// back as `None`. Keeps optional keys visible in the generated config.
mod empty_is_none {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S, T>(value: &Option<T>, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        T: Serialize,
    {
        match value {
            Some(v) => v.serialize(s),
            None => s.serialize_str(""),
        }
    }

    pub fn deserialize<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
    where
        D: Deserializer<'de>,
        T: From<String>,
    {
        let raw = String::deserialize(d)?;
        Ok(if raw.trim().is_empty() { None } else { Some(T::from(raw)) })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ModesConfig {
    /// Local time, "HH:MM". The window may wrap past midnight.
    pub learning_window_start: String,
    pub learning_window_end: String,
    /// Learning waits until the user has been quiet this long.
    pub quiet_minutes_before_learning: u32,
    /// Stop the inference server after this much inactivity in Idle mode.
    pub idle_unload_after_minutes: u32,
    /// How often the scheduler wakes to reconsider.
    pub tick_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BudgetsConfig {
    pub learning_tokens_per_day: u64,
    pub http_requests_per_day: u64,
    pub http_bytes_per_day: u64,
    pub learning_minutes_per_day: u64,
    pub research_jobs_per_day: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkConfig {
    pub enabled: bool,
    /// In chat mode, URLs the user typed may be fetched even if off-list.
    pub allow_user_provided_urls: bool,
    /// Domain suffixes the agent may fetch on its own initiative.
    pub allowed_domains: Vec<String>,
    pub max_fetch_bytes: u64,
    pub fetch_timeout_secs: u64,
    pub user_agent: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ContextConfig {
    /// Rough cap on the system prompt (characters, ~4 per token).
    pub max_system_chars: usize,
    /// Prior conversation turns replayed into each chat request.
    pub max_history_turns: usize,
    pub max_tool_output_chars: usize,
    /// Tool-call rounds allowed per user message.
    pub max_tool_rounds: usize,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            data_dir: default_app_dir().join("data"),
            identity: IdentityConfig::default(),
            models: ModelsConfig::default(),
            modes: ModesConfig::default(),
            budgets: BudgetsConfig::default(),
            network: NetworkConfig::default(),
            context: ContextConfig::default(),
        }
    }
}

impl Default for IdentityConfig {
    fn default() -> Self {
        Self {
            name: "Aethra".to_string(),
        }
    }
}

impl Default for ModelsConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://127.0.0.1:8080".to_string(),
            model_name: "local".to_string(),
            api_key: None,
            request_timeout_secs: 900,
            sidecar: SidecarConfig::default(),
            chat: ModelProfile {
                model_path: None,
                ctx_size: 16384,
                threads: 8,
                gpu_layers: 0,
                extra_args: Vec::new(),
                temperature: 0.7,
                max_tokens: 2048,
            },
            learning: ModelProfile {
                model_path: None,
                ctx_size: 16384,
                threads: 8,
                gpu_layers: 0,
                extra_args: Vec::new(),
                temperature: 0.4,
                max_tokens: 2048,
            },
        }
    }
}

impl Default for SidecarConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            executable: None,
            port: 8080,
            startup_timeout_secs: 300,
        }
    }
}

impl Default for ModelProfile {
    fn default() -> Self {
        Self {
            model_path: None,
            ctx_size: 16384,
            threads: 8,
            gpu_layers: 0,
            extra_args: Vec::new(),
            temperature: 0.7,
            max_tokens: 2048,
        }
    }
}

impl Default for ModesConfig {
    fn default() -> Self {
        Self {
            learning_window_start: "01:00".to_string(),
            learning_window_end: "07:00".to_string(),
            quiet_minutes_before_learning: 20,
            idle_unload_after_minutes: 15,
            tick_seconds: 30,
        }
    }
}

impl Default for BudgetsConfig {
    fn default() -> Self {
        Self {
            learning_tokens_per_day: 150_000,
            http_requests_per_day: 60,
            http_bytes_per_day: 40 * 1024 * 1024,
            learning_minutes_per_day: 240,
            research_jobs_per_day: 6,
        }
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            allow_user_provided_urls: true,
            allowed_domains: vec![
                "wikipedia.org",
                "wikibooks.org",
                "wiktionary.org",
                "docs.rs",
                "doc.rust-lang.org",
                "rust-lang.org",
                "developer.mozilla.org",
                "arxiv.org",
                "sqlite.org",
                "tauri.app",
                "w3.org",
                "rfc-editor.org",
                "datatracker.ietf.org",
                "github.com",
                "raw.githubusercontent.com",
                "plato.stanford.edu",
                "ncbi.nlm.nih.gov",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            max_fetch_bytes: 2 * 1024 * 1024,
            fetch_timeout_secs: 20,
            user_agent: "Aethra/0.1 (+https://github.com/PanteraPolnocy/Project-Aethra)".to_string(),
        }
    }
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            max_system_chars: 14_000,
            max_history_turns: 12,
            max_tool_output_chars: 12_000,
            max_tool_rounds: 4,
        }
    }
}

pub fn default_app_dir() -> PathBuf {
    dirs::data_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_DIR_NAME)
}

pub fn default_config_path() -> PathBuf {
    default_app_dir().join(CONFIG_FILE_NAME)
}

impl AppConfig {
    /// Reads the config, or writes the defaults and returns them on first run.
    pub fn load_or_create(path: &Path) -> Result<Self> {
        if path.is_file() {
            let text = std::fs::read_to_string(path)?;
            let cfg: AppConfig =
                toml::from_str(&text).map_err(|e| CoreError::Config(format!("{}: {e}", path.display())))?;
            cfg.validate()?;
            return Ok(cfg);
        }
        let cfg = AppConfig::default();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(&cfg).map_err(|e| CoreError::Config(e.to_string()))?;
        std::fs::write(path, format!("{}\n{text}", CONFIG_HEADER))?;
        Ok(cfg)
    }

    pub fn validate(&self) -> Result<()> {
        parse_hhmm(&self.modes.learning_window_start)
            .ok_or_else(|| CoreError::Config("modes.learning_window_start must be HH:MM".into()))?;
        parse_hhmm(&self.modes.learning_window_end)
            .ok_or_else(|| CoreError::Config("modes.learning_window_end must be HH:MM".into()))?;
        if self.models.sidecar.enabled {
            if self.models.sidecar.executable.is_none() {
                return Err(CoreError::Config(
                    "models.sidecar.enabled is true but models.sidecar.executable is unset".into(),
                ));
            }
            if self.models.chat.model_path.is_none() {
                return Err(CoreError::Config(
                    "models.sidecar.enabled is true but models.chat.model_path is unset".into(),
                ));
            }
        }
        Ok(())
    }

    pub fn effective_endpoint(&self) -> String {
        if self.models.sidecar.enabled {
            format!("http://127.0.0.1:{}", self.models.sidecar.port)
        } else {
            self.models.endpoint.clone()
        }
    }

    pub fn mind_db_path(&self) -> PathBuf {
        self.data_dir.join("mind.db")
    }

    pub fn episodes_db_path(&self) -> PathBuf {
        self.data_dir.join("episodes.db")
    }

    pub fn cache_db_path(&self) -> PathBuf {
        self.data_dir.join("cache.db")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }

    pub fn workspace_dir(&self) -> PathBuf {
        self.data_dir.join("workspace")
    }

    /// A copy safe to show in the UI: secrets removed.
    pub fn redacted(&self) -> AppConfig {
        let mut c = self.clone();
        if c.models.api_key.is_some() {
            c.models.api_key = Some("<set>".to_string());
        }
        c
    }

    /// Flat key/value view for the Settings screen.
    pub fn summary(&self) -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        m.insert("data_dir".into(), self.data_dir.display().to_string());
        m.insert("models.endpoint".into(), self.effective_endpoint());
        m.insert("models.sidecar.enabled".into(), self.models.sidecar.enabled.to_string());
        m.insert(
            "models.sidecar.executable".into(),
            self.models
                .sidecar
                .executable
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "<unset>".into()),
        );
        m.insert(
            "models.chat.model_path".into(),
            self.models
                .chat
                .model_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "<unset>".into()),
        );
        m.insert("models.chat.gpu_layers".into(), self.models.chat.gpu_layers.to_string());
        m.insert("models.learning.gpu_layers".into(), self.models.learning.gpu_layers.to_string());
        m.insert(
            "modes.learning_window".into(),
            format!("{} - {}", self.modes.learning_window_start, self.modes.learning_window_end),
        );
        m.insert(
            "modes.quiet_minutes_before_learning".into(),
            self.modes.quiet_minutes_before_learning.to_string(),
        );
        m.insert("network.enabled".into(), self.network.enabled.to_string());
        m.insert("network.allowed_domains".into(), self.network.allowed_domains.join(", "));
        m
    }
}

/// Parses "HH:MM" into minutes since midnight.
pub fn parse_hhmm(s: &str) -> Option<u32> {
    let (h, m) = s.trim().split_once(':')?;
    let h: u32 = h.parse().ok()?;
    let m: u32 = m.parse().ok()?;
    if h > 23 || m > 59 {
        return None;
    }
    Some(h * 60 + m)
}

const CONFIG_HEADER: &str = r#"# Aethra configuration.
#
# This file is read once at startup and is never written by the agent.
# To run a local model, either:
#   (a) set models.sidecar.enabled = true, models.sidecar.executable to your
#       llama-server binary and models.chat.model_path to a GGUF file, or
#   (b) start any OpenAI-compatible server yourself and point models.endpoint at it.
# Where to download llama-server and a model: README, "Local model (CPU-first)".
# Use single-quoted TOML strings for Windows paths so backslashes survive.
# Keys written as "" are optional and unset; models.learning.model_path = ""
# means "same model as chat".
#
# CPU-first defaults: gpu_layers = 0 keeps the GPU free. For faster chat you may
# raise models.chat.gpu_layers; learning stays on the CPU overnight.
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hhmm() {
        assert_eq!(parse_hhmm("01:00"), Some(60));
        assert_eq!(parse_hhmm("23:59"), Some(23 * 60 + 59));
        assert_eq!(parse_hhmm("24:00"), None);
        assert_eq!(parse_hhmm("nope"), None);
    }

    #[test]
    fn defaults_validate() {
        AppConfig::default().validate().expect("defaults must be valid");
    }

    #[test]
    fn defaults_round_trip_through_toml() {
        let cfg = AppConfig::default();
        let text = toml::to_string_pretty(&cfg).unwrap();
        let back: AppConfig = toml::from_str(&text).unwrap();
        assert_eq!(back.network.allowed_domains, cfg.network.allowed_domains);
        assert_eq!(back.modes.learning_window_start, cfg.modes.learning_window_start);
    }

    #[test]
    fn optional_keys_are_written_and_read_back_as_unset() {
        let text = toml::to_string_pretty(&AppConfig::default()).unwrap();
        assert!(text.contains("executable = \"\""), "{text}");
        assert!(text.contains("model_path = \"\""), "{text}");
        assert!(text.contains("api_key = \"\""), "{text}");
        assert!(text.contains("temperature = 0.7"), "{text}");
        assert!(!text.contains("0.699999"), "{text}");

        let back: AppConfig = toml::from_str(&text).unwrap();
        assert!(back.models.sidecar.executable.is_none());
        assert!(back.models.chat.model_path.is_none());
        assert!(back.models.api_key.is_none());

        let filled: AppConfig = toml::from_str(
            r#"
[models.sidecar]
executable = 'D:\llama\llama-server.exe'
[models.chat]
model_path = 'D:\models\x.gguf'
[models.learning]
model_path = '   '
"#,
        )
        .unwrap();
        assert_eq!(filled.models.sidecar.executable.as_deref(), Some(Path::new(r"D:\llama\llama-server.exe")));
        assert_eq!(filled.models.chat.model_path.as_deref(), Some(Path::new(r"D:\models\x.gguf")));
        assert!(filled.models.learning.model_path.is_none());
    }
}
