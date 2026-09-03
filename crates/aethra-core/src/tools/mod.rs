//! Tools the model may call. Each tool declares its schema, checks policy
//! itself, and labels its output with provenance so the caller knows how much
//! to trust it.

pub mod html;
pub mod web_fetch;

use std::collections::HashSet;
use std::sync::Arc;

use aethra_models::ToolSpec;
use async_trait::async_trait;

use crate::episodes::Taint;
use crate::error::Result;
use crate::knowledge::NoteSource;
use crate::mode::Mode;

pub struct ToolContext {
    pub mode: Mode,
    /// Normalised URLs the user typed in the current message (chat mode only).
    pub user_urls: HashSet<String>,
    pub episode_id: String,
}

pub struct ToolOutput {
    pub content: String,
    pub taint: Taint,
    pub sources: Vec<NoteSource>,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn spec(&self) -> ToolSpec;
    async fn call(&self, args: serde_json::Value, ctx: &ToolContext) -> Result<ToolOutput>;
}

#[derive(Default)]
pub struct ToolRegistry {
    tools: Vec<Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.push(tool);
    }

    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools.iter().map(|t| t.spec()).collect()
    }

    pub fn names(&self) -> Vec<String> {
        self.tools.iter().map(|t| t.spec().name).collect()
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.iter().find(|t| t.spec().name == name).cloned()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}
