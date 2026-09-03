//! Client for any server speaking the OpenAI chat-completions dialect.
//! Tested target: llama.cpp `llama-server` started with `--jinja`, which
//! parses tool calls and honours `response_format = json_schema`.

use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    ChatMessage, Completion, CompletionRequest, Embedder, LanguageModel, ModelError, Role, ToolCall, ToolSpec,
    Usage,
};

#[derive(Debug, Clone)]
pub struct OpenAiCompatConfig {
    pub id: String,
    pub base_url: String,
    pub model_name: String,
    pub api_key: Option<String>,
    pub request_timeout: Duration,
}

pub struct OpenAiCompatModel {
    cfg: OpenAiCompatConfig,
    client: reqwest::Client,
}

impl OpenAiCompatModel {
    pub fn new(cfg: OpenAiCompatConfig) -> Result<Self, ModelError> {
        let client = reqwest::Client::builder()
            .timeout(cfg.request_timeout)
            .connect_timeout(Duration::from_secs(5))
            .build()
            .map_err(|e| ModelError::Config(e.to_string()))?;
        let base_url = cfg.base_url.trim_end_matches('/').to_string();
        Ok(Self {
            cfg: OpenAiCompatConfig { base_url, ..cfg },
            client,
        })
    }

    pub fn base_url(&self) -> &str {
        &self.cfg.base_url
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}{}", self.cfg.base_url, path)
    }

    fn apply_auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.cfg.api_key {
            Some(key) if !key.is_empty() => req.bearer_auth(key),
            _ => req,
        }
    }

    async fn post_json<T: for<'de> Deserialize<'de>>(&self, path: &str, body: &impl Serialize) -> Result<T, ModelError> {
        let req = self.apply_auth(self.client.post(self.endpoint(path)).json(body));
        let resp = req.send().await.map_err(classify_transport)?;
        let status = resp.status();
        let text = resp.text().await.map_err(classify_transport)?;
        if !status.is_success() {
            return Err(ModelError::Http {
                status: status.as_u16(),
                body: truncate(&text, 2000),
            });
        }
        serde_json::from_str::<T>(&text).map_err(|e| ModelError::Malformed(format!("{e}: {}", truncate(&text, 500))))
    }
}

fn classify_transport(e: reqwest::Error) -> ModelError {
    if e.is_timeout() {
        ModelError::Timeout
    } else if e.is_connect() || e.is_request() {
        ModelError::Unreachable(e.to_string())
    } else {
        ModelError::Malformed(e.to_string())
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}...")
    }
}

// --- wire format -----------------------------------------------------------

#[derive(Serialize)]
struct WireRequest<'a> {
    model: &'a str,
    messages: Vec<WireMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<WireTool<'a>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    stream: bool,
}

#[derive(Serialize)]
struct WireMessage {
    role: &'static str,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<WireToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct WireToolCall {
    #[serde(default)]
    id: String,
    #[serde(default, rename = "type")]
    kind: String,
    function: WireFunctionCall,
}

#[derive(Serialize, Deserialize)]
struct WireFunctionCall {
    name: String,
    #[serde(default)]
    arguments: String,
}

#[derive(Serialize)]
struct WireTool<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    function: &'a ToolSpec,
}

#[derive(Deserialize)]
struct WireResponse {
    #[serde(default)]
    choices: Vec<WireChoice>,
    #[serde(default)]
    usage: Option<WireUsage>,
}

#[derive(Deserialize)]
struct WireChoice {
    message: WireResponseMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct WireResponseMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<WireToolCall>>,
}

#[derive(Deserialize)]
struct WireUsage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
}

#[derive(Serialize)]
struct WireEmbedRequest<'a> {
    model: &'a str,
    input: &'a [String],
}

#[derive(Deserialize)]
struct WireEmbedResponse {
    data: Vec<WireEmbedding>,
}

#[derive(Deserialize)]
struct WireEmbedding {
    #[serde(default)]
    index: usize,
    embedding: Vec<f32>,
}

fn role_str(role: Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    }
}

fn to_wire_message(m: &ChatMessage) -> WireMessage {
    let tool_calls = if m.tool_calls.is_empty() {
        None
    } else {
        Some(
            m.tool_calls
                .iter()
                .map(|tc| WireToolCall {
                    id: tc.id.clone(),
                    kind: "function".to_string(),
                    function: WireFunctionCall {
                        name: tc.name.clone(),
                        arguments: tc.arguments.to_string(),
                    },
                })
                .collect(),
        )
    };
    WireMessage {
        role: role_str(m.role),
        content: m.content.clone(),
        tool_calls,
        tool_call_id: m.tool_call_id.clone(),
    }
}

fn from_wire_tool_calls(calls: Vec<WireToolCall>) -> Vec<ToolCall> {
    calls
        .into_iter()
        .enumerate()
        .map(|(i, c)| {
            let id = if c.id.is_empty() { format!("call_{i}") } else { c.id };
            let arguments = if c.function.arguments.trim().is_empty() {
                serde_json::Value::Object(Default::default())
            } else {
                serde_json::from_str(&c.function.arguments)
                    .unwrap_or_else(|_| serde_json::Value::String(c.function.arguments.clone()))
            };
            ToolCall {
                id,
                name: c.function.name,
                arguments,
            }
        })
        .collect()
}

#[async_trait]
impl LanguageModel for OpenAiCompatModel {
    fn id(&self) -> &str {
        &self.cfg.id
    }

    async fn complete(&self, request: CompletionRequest) -> Result<Completion, ModelError> {
        let tools = if request.tools.is_empty() {
            None
        } else {
            Some(
                request
                    .tools
                    .iter()
                    .map(|t| WireTool {
                        kind: "function",
                        function: t,
                    })
                    .collect(),
            )
        };
        let response_format = request.json_schema.as_ref().map(|s| {
            serde_json::json!({
                "type": "json_schema",
                "json_schema": { "name": s.name, "schema": s.schema, "strict": true }
            })
        });
        let body = WireRequest {
            model: &self.cfg.model_name,
            messages: request.messages.iter().map(to_wire_message).collect(),
            tools,
            response_format,
            max_tokens: request.max_tokens,
            temperature: request.temperature,
            stream: false,
        };

        let resp: WireResponse = self.post_json("/v1/chat/completions", &body).await?;
        let choice = resp
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| ModelError::Malformed("response contained no choices".into()))?;
        let usage = resp
            .usage
            .map(|u| Usage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
            })
            .unwrap_or_default();

        Ok(Completion {
            message: ChatMessage {
                role: Role::Assistant,
                content: choice.message.content.unwrap_or_default(),
                tool_calls: choice.message.tool_calls.map(from_wire_tool_calls).unwrap_or_default(),
                tool_call_id: None,
            },
            usage,
            finish_reason: choice.finish_reason,
        })
    }

    async fn health(&self) -> Result<(), ModelError> {
        let req = self.apply_auth(self.client.get(self.endpoint("/v1/models")));
        let resp = req.send().await.map_err(classify_transport)?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(ModelError::Http {
                status: resp.status().as_u16(),
                body: String::new(),
            })
        }
    }
}

#[async_trait]
impl Embedder for OpenAiCompatModel {
    fn id(&self) -> &str {
        &self.cfg.id
    }

    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, ModelError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let body = WireEmbedRequest {
            model: &self.cfg.model_name,
            input: texts,
        };
        let resp: WireEmbedResponse = self.post_json("/v1/embeddings", &body).await?;
        let mut data = resp.data;
        data.sort_by_key(|d| d.index);
        if data.len() != texts.len() {
            return Err(ModelError::Malformed(format!(
                "expected {} embeddings, received {}",
                texts.len(),
                data.len()
            )));
        }
        Ok(data.into_iter().map(|d| d.embedding).collect())
    }
}
