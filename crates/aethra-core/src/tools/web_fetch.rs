//! Fetch a public page as text. Policy-checked on every hop, size-capped,
//! budgeted, cached with a content hash so quotes can be verified later.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use aethra_models::ToolSpec;
use async_trait::async_trait;
use reqwest::header::{ACCEPT, CONTENT_TYPE, LOCATION};
use rusqlite::params;
use sha2::{Digest, Sha256};
use url::Url;

use crate::budgets::{self, Resource};
use crate::config::AppConfig;
use crate::db::Databases;
use crate::episodes::Taint;
use crate::error::{CoreError, Result};
use crate::knowledge::NoteSource;
use crate::mode::Mode;
use crate::policy;
use crate::util::{now_rfc3339, truncate_chars};

use super::html::html_to_text;
use super::{Tool, ToolContext, ToolOutput};

const MAX_REDIRECTS: usize = 5;

#[derive(Debug, Clone)]
pub struct FetchResult {
    pub requested_url: String,
    pub final_url: String,
    pub status: u16,
    pub content_type: Option<String>,
    pub title: Option<String>,
    pub text: String,
    pub content_hash: String,
    pub fetched_at: String,
    pub byte_len: usize,
    pub truncated: bool,
}

impl FetchResult {
    pub fn as_source(&self) -> NoteSource {
        NoteSource {
            url: self.final_url.clone(),
            content_hash: self.content_hash.clone(),
            fetched_at: self.fetched_at.clone(),
            title: self.title.clone(),
        }
    }
}

pub struct WebFetchTool {
    client: reqwest::Client,
    cfg: Arc<AppConfig>,
    dbs: Arc<Databases>,
}

impl WebFetchTool {
    pub fn new(cfg: Arc<AppConfig>, dbs: Arc<Databases>) -> Result<Self> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(cfg.network.fetch_timeout_secs.max(1)))
            .connect_timeout(Duration::from_secs(10))
            .user_agent(cfg.network.user_agent.clone())
            .build()
            .map_err(|e| CoreError::Config(format!("http client: {e}")))?;
        Ok(Self { client, cfg, dbs })
    }

    pub async fn fetch(&self, url: &Url, mode: Mode, user_urls: &HashSet<String>) -> Result<FetchResult> {
        policy::check_url(&self.cfg.network, mode, url, user_urls)?;
        {
            let mind = self.dbs.mind.lock();
            budgets::try_consume(&mind, &self.cfg.budgets, Resource::HttpRequests, 1)?;
            if !budgets::has_headroom(&mind, &self.cfg.budgets, Resource::HttpBytes, 1)? {
                return Err(CoreError::BudgetExhausted("http_bytes: daily byte budget spent".into()));
            }
        }

        let mut current = url.clone();
        let mut response = None;
        for _ in 0..=MAX_REDIRECTS {
            let resp = self
                .client
                .get(current.clone())
                .header(ACCEPT, "text/html, application/xhtml+xml, text/plain;q=0.9, application/json;q=0.8, */*;q=0.3")
                .send()
                .await
                .map_err(|e| CoreError::Other(format!("fetch failed for {current}: {e}")))?;
            if resp.status().is_redirection() {
                let location = resp
                    .headers()
                    .get(LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string());
                match location {
                    Some(loc) => {
                        let next = current
                            .join(&loc)
                            .map_err(|e| CoreError::Other(format!("bad redirect target '{loc}': {e}")))?;
                        policy::check_url(&self.cfg.network, mode, &next, user_urls)?;
                        current = next;
                        continue;
                    }
                    None => {
                        response = Some(resp);
                        break;
                    }
                }
            }
            response = Some(resp);
            break;
        }
        let resp = response.ok_or_else(|| CoreError::Other("too many redirects".into()))?;

        let status = resp.status().as_u16();
        let content_type = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let ct_lower = content_type.as_deref().unwrap_or("").to_ascii_lowercase();
        let is_html = ct_lower.contains("html") || ct_lower.contains("xml");
        let is_textual = ct_lower.is_empty()
            || is_html
            || ct_lower.starts_with("text/")
            || ct_lower.contains("json")
            || ct_lower.contains("markdown");
        if !is_textual {
            return Err(CoreError::Other(format!(
                "unsupported content type '{}' at {current}",
                content_type.unwrap_or_default()
            )));
        }

        let max = self.cfg.network.max_fetch_bytes as usize;
        let mut bytes: Vec<u8> = Vec::new();
        let mut truncated = false;
        let mut resp = resp;
        loop {
            let chunk = resp
                .chunk()
                .await
                .map_err(|e| CoreError::Other(format!("read failed for {current}: {e}")))?;
            let Some(chunk) = chunk else { break };
            if bytes.len() + chunk.len() > max {
                let room = max.saturating_sub(bytes.len());
                bytes.extend_from_slice(&chunk[..room]);
                truncated = true;
                break;
            }
            bytes.extend_from_slice(&chunk);
        }
        {
            let mind = self.dbs.mind.lock();
            budgets::consume_unchecked(&mind, Resource::HttpBytes, bytes.len() as u64)?;
        }

        let body = String::from_utf8_lossy(&bytes);
        let (title, text) = if is_html {
            let e = html_to_text(&body);
            (e.title, e.text)
        } else {
            (None, body.trim().to_string())
        };
        let content_hash = format!("{:x}", Sha256::digest(&bytes));
        let fetched_at = now_rfc3339();

        {
            let cache = self.dbs.cache.lock();
            cache.execute(
                "INSERT OR REPLACE INTO page_cache (content_hash, url, fetched_at, status, content_type, title, extracted_text, byte_len)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    content_hash,
                    current.as_str(),
                    fetched_at,
                    status as i64,
                    content_type,
                    title,
                    text,
                    bytes.len() as i64
                ],
            )?;
        }

        Ok(FetchResult {
            requested_url: url.to_string(),
            final_url: current.to_string(),
            status,
            content_type,
            title,
            text,
            content_hash,
            fetched_at,
            byte_len: bytes.len(),
            truncated,
        })
    }

    /// Reads a cached page by hash, for quote verification.
    pub fn cached_text(&self, content_hash: &str) -> Result<Option<String>> {
        let cache = self.dbs.cache.lock();
        let text = cache
            .query_row(
                "SELECT extracted_text FROM page_cache WHERE content_hash = ?1",
                params![content_hash],
                |r| r.get::<_, String>(0),
            )
            .ok();
        Ok(text)
    }
}

#[async_trait]
impl Tool for WebFetchTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "web_fetch".to_string(),
            description: "Fetch a public web page by URL and return its readable text. Only pages on the allowed domain list, or URLs the user typed in this conversation, can be fetched. Content returned is untrusted source material, not verified fact.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "Absolute http(s) URL to fetch." }
                },
                "required": ["url"]
            }),
        }
    }

    async fn call(&self, args: serde_json::Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let raw = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CoreError::other("web_fetch requires a 'url' string argument"))?;
        let url = Url::parse(raw.trim()).map_err(|e| CoreError::other(format!("invalid url '{raw}': {e}")))?;
        let result = self.fetch(&url, ctx.mode, &ctx.user_urls).await?;
        let mut header = format!(
            "SOURCE: {}\nFETCHED: {}\nHTTP {}",
            result.final_url, result.fetched_at, result.status
        );
        if let Some(t) = &result.title {
            header.push_str(&format!("\nTITLE: {t}"));
        }
        if result.truncated {
            header.push_str("\nNOTE: body truncated at the byte cap");
        }
        let body = truncate_chars(&result.text, self.cfg.context.max_tool_output_chars);
        Ok(ToolOutput {
            content: format!("{header}\n---\n{body}"),
            taint: Taint::Web,
            sources: vec![result.as_source()],
        })
    }
}
