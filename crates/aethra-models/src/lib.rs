//! Language-model abstraction for Aethra.
//!
//! The model is a service the mind talks to, never the mind itself. Everything
//! here is provider-agnostic: any OpenAI-compatible endpoint (llama-server,
//! mistral.rs, Ollama, a remote provider) can sit behind `LanguageModel`.

pub mod openai_compat;
pub mod sidecar;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// A tool invocation requested by the model. `arguments` is already parsed;
/// providers that return a JSON string are normalised before reaching callers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self::plain(Role::System, content)
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self::plain(Role::User, content)
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self::plain(Role::Assistant, content)
    }

    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: Some(tool_call_id.into()),
        }
    }

    fn plain(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }
}

/// Description of a callable tool, in JSON-schema form for `parameters`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// A named JSON schema the model must conform to (constrained decoding).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonSchemaSpec {
    pub name: String,
    pub schema: serde_json::Value,
}

#[derive(Debug, Clone, Default)]
pub struct CompletionRequest {
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<ToolSpec>,
    pub json_schema: Option<JsonSchemaSpec>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

impl Usage {
    pub fn total(&self) -> u32 {
        self.prompt_tokens.saturating_add(self.completion_tokens)
    }

    pub fn add(&mut self, other: Usage) {
        self.prompt_tokens = self.prompt_tokens.saturating_add(other.prompt_tokens);
        self.completion_tokens = self.completion_tokens.saturating_add(other.completion_tokens);
    }
}

#[derive(Debug, Clone)]
pub struct Completion {
    pub message: ChatMessage,
    pub usage: Usage,
    pub finish_reason: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("model endpoint unreachable: {0}")]
    Unreachable(String),
    #[error("model endpoint returned HTTP {status}: {body}")]
    Http { status: u16, body: String },
    #[error("malformed model response: {0}")]
    Malformed(String),
    #[error("model request timed out")]
    Timeout,
    #[error("model configuration error: {0}")]
    Config(String),
}

#[async_trait]
pub trait LanguageModel: Send + Sync {
    /// Stable identifier used in logs and provenance (for example "local:llama-server").
    fn id(&self) -> &str;

    async fn complete(&self, request: CompletionRequest) -> Result<Completion, ModelError>;

    /// Cheap liveness probe. Must not load or unload anything.
    async fn health(&self) -> Result<(), ModelError>;
}

#[async_trait]
pub trait Embedder: Send + Sync {
    fn id(&self) -> &str;

    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, ModelError>;
}
