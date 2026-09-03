//! Learning-mode jobs. Two kinds in this phase:
//!
//! - Consolidate: turn unconsolidated episodes into a first-person summary and
//!   open questions (Tier A writes).
//! - Research: take the most promising open question, fetch a few allowlisted
//!   pages, write a note whose quotes are verified against the cached text.
//!
//! Both are interruptible between steps and during model calls.

use std::collections::HashSet;

use aethra_models::{ChatMessage, CompletionRequest, JsonSchemaSpec, LanguageModel, Usage};
use rusqlite::{params, Connection};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::sync::Arc;
use url::Url;

use crate::budgets::{self, Resource};
use crate::episodes::{self, EpisodeKind, Taint};
use crate::error::{CoreError, Result};
use crate::events::MindEvent;
use crate::knowledge::{self, NoteSource, Question};
use crate::mind::Mind;
use crate::mode::Mode;
use crate::model_host::Profile;
use crate::state::StateEvent;
use crate::tools::web_fetch::FetchResult;
use crate::util::{now_rfc3339, truncate_chars};

const CONSOLIDATE_BATCH: u32 = 30;
const DIGEST_CHAR_CAP: usize = 24_000;
const RESEARCH_MATERIAL_CHAR_CAP: usize = 30_000;
const MAX_RESEARCH_URLS: usize = 3;
const MIN_QUOTE_CHARS: usize = 12;

pub enum JobKind {
    Consolidate,
    Research(Question),
}

impl JobKind {
    pub fn kind_str(&self) -> &'static str {
        match self {
            JobKind::Consolidate => "consolidate",
            JobKind::Research(_) => "research",
        }
    }

    pub fn detail(&self) -> String {
        match self {
            JobKind::Consolidate => "consolidate recent episodes into memory".to_string(),
            JobKind::Research(q) => truncate_chars(&q.text, 160),
        }
    }

    fn payload(&self) -> serde_json::Value {
        match self {
            JobKind::Consolidate => serde_json::json!({}),
            JobKind::Research(q) => serde_json::json!({ "question_id": q.id, "text": q.text }),
        }
    }

    fn repeat_hash(&self) -> String {
        match self {
            JobKind::Consolidate => "consolidate".to_string(),
            JobKind::Research(q) => format!("research:{}", q.id),
        }
    }
}

pub struct JobOutcome {
    pub summary: String,
    pub success: bool,
}

pub fn pick_job(mind: &Mind) -> Result<Option<JobKind>> {
    let pending = mind.with_episodes(episodes::count_unconsolidated)?;
    if pending > 0 {
        return Ok(Some(JobKind::Consolidate));
    }
    if mind.web.is_none() {
        return Ok(None);
    }
    let question = mind.with_mind(|c| {
        if !budgets::has_headroom(c, &mind.cfg.budgets, Resource::ResearchJobs, 1)? {
            return Ok(None);
        }
        if !budgets::has_headroom(c, &mind.cfg.budgets, Resource::HttpRequests, MAX_RESEARCH_URLS as u64)? {
            return Ok(None);
        }
        knowledge::next_open_question(c, 3)
    })?;
    Ok(question.map(JobKind::Research))
}

pub fn insert_job(conn: &Connection, id: &str, job: &JobKind) -> Result<()> {
    conn.execute(
        "INSERT INTO jobs (id, kind, payload, state, created_at, started_at, repeat_hash)
         VALUES (?1, ?2, ?3, 'running', ?4, ?4, ?5)",
        params![id, job.kind_str(), job.payload().to_string(), now_rfc3339(), job.repeat_hash()],
    )?;
    Ok(())
}

pub fn finish_job(conn: &Connection, id: &str, state: &str, outcome: Option<&str>, error: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE jobs SET state = ?1, finished_at = ?2, outcome = ?3, error = ?4 WHERE id = ?5",
        params![state, now_rfc3339(), outcome, error, id],
    )?;
    Ok(())
}

pub async fn run_job(mind: &Mind, job_id: &str, job: JobKind) -> Result<JobOutcome> {
    let model = mind.host.ensure(Profile::Learning).await?;
    match job {
        JobKind::Consolidate => consolidate(mind, &model, job_id).await,
        JobKind::Research(q) => research(mind, &model, job_id, q).await,
    }
}

// --- shared helpers ----------------------------------------------------------

#[derive(Deserialize)]
struct QuestionOut {
    text: String,
    #[serde(default = "default_importance")]
    importance: f64,
    #[serde(default)]
    #[allow(dead_code)]
    why: String,
}

fn default_importance() -> f64 {
    0.5
}

fn strip_fences(s: &str) -> &str {
    let s = s.trim();
    match s.strip_prefix("```") {
        Some(rest) => {
            let rest = rest.trim_start_matches(|c: char| c.is_ascii_alphanumeric());
            let rest = rest.trim_start();
            rest.strip_suffix("```").unwrap_or(rest).trim()
        }
        None => s,
    }
}

/// Tolerant JSON extraction: accepts fenced output and leading prose.
pub fn parse_json<T: DeserializeOwned>(raw: &str) -> Result<T> {
    let s = strip_fences(raw);
    if let Ok(v) = serde_json::from_str::<T>(s) {
        return Ok(v);
    }
    if let (Some(a), Some(b)) = (s.find('{'), s.rfind('}')) {
        if b > a {
            if let Ok(v) = serde_json::from_str::<T>(&s[a..=b]) {
                return Ok(v);
            }
        }
    }
    Err(CoreError::other(format!(
        "model output was not the expected JSON: {}",
        truncate_chars(s, 300)
    )))
}

fn normalise_for_match(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Whitespace- and case-insensitive containment check for quote verification.
pub fn contains_quote(haystack: &str, quote: &str) -> bool {
    let q = normalise_for_match(quote);
    if q.chars().count() < MIN_QUOTE_CHARS {
        return false;
    }
    normalise_for_match(haystack).contains(&q)
}

fn learning_request(mind: &Mind, messages: Vec<ChatMessage>, schema: JsonSchemaSpec) -> CompletionRequest {
    let profile = mind.host.profile_config(Profile::Learning);
    CompletionRequest {
        messages,
        tools: Vec::new(),
        json_schema: Some(schema),
        max_tokens: Some(profile.max_tokens),
        temperature: Some(profile.temperature as f32),
    }
}

fn record_tokens(mind: &Mind, usage: Usage) {
    let total = u64::from(usage.total());
    if total > 0 {
        let _ = mind.with_mind(|c| budgets::consume_unchecked(c, Resource::LearningTokens, total));
    }
}

fn short(id: &str) -> &str {
    &id[..id.len().min(8)]
}

// --- consolidate -------------------------------------------------------------

#[derive(Deserialize)]
struct ConsolidationOut {
    summary: String,
    #[serde(default)]
    observations: Vec<String>,
    #[serde(default)]
    open_questions: Vec<QuestionOut>,
    #[serde(default)]
    self_observations: Vec<String>,
}

fn consolidation_schema() -> JsonSchemaSpec {
    JsonSchemaSpec {
        name: "consolidation".to_string(),
        schema: serde_json::json!({
            "type": "object",
            "properties": {
                "summary": { "type": "string" },
                "observations": { "type": "array", "items": { "type": "string" } },
                "open_questions": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "text": { "type": "string" },
                            "importance": { "type": "number" },
                            "why": { "type": "string" }
                        },
                        "required": ["text", "importance", "why"]
                    }
                },
                "self_observations": { "type": "array", "items": { "type": "string" } }
            },
            "required": ["summary", "observations", "open_questions", "self_observations"]
        }),
    }
}

async fn consolidate(mind: &Mind, model: &Arc<dyn LanguageModel>, job_id: &str) -> Result<JobOutcome> {
    let candidates = mind.with_episodes(|c| episodes::unconsolidated(c, CONSOLIDATE_BATCH))?;
    if candidates.is_empty() {
        return Ok(JobOutcome {
            summary: "nothing to consolidate".to_string(),
            success: true,
        });
    }

    let mut digest = String::new();
    let mut taint = Taint::Internal;
    let mut ids: Vec<String> = Vec::new();
    let mut period_start = candidates[0].started_at.clone();
    let mut period_end = period_start.clone();
    for ep in &candidates {
        taint = taint.escalate(Taint::parse(&ep.taint));
        ids.push(ep.id.clone());
        if ep.started_at < period_start {
            period_start = ep.started_at.clone();
        }
        let end = ep.ended_at.clone().unwrap_or_else(|| ep.started_at.clone());
        if end > period_end {
            period_end = end;
        }
        let items = mind.with_episodes(|c| episodes::items(c, &ep.id))?;
        digest.push_str(&format!(
            "\n### Episode {} ({}, mode {}, taint {}) at {}\n",
            short(&ep.id),
            ep.kind,
            ep.mode,
            ep.taint,
            ep.started_at
        ));
        if !ep.summary.is_empty() {
            digest.push_str(&format!("Summary: {}\n", ep.summary));
        }
        for it in items {
            let label = match &it.tool_name {
                Some(t) => format!("{}:{t}", it.role),
                None => it.role.clone(),
            };
            digest.push_str(&format!(
                "[{label}] {}\n",
                truncate_chars(&it.content, 700).replace('\n', " ")
            ));
        }
        if digest.len() > DIGEST_CHAR_CAP {
            break;
        }
    }

    let episode_id = mind.with_episodes(|c| {
        episodes::begin(c, EpisodeKind::Learning, Mode::Learning, Some(job_id), taint)
    })?;

    let name = &mind.cfg.identity.name;
    let system = format!(
        "You are {name}, consolidating your own recent experience into long-term memory. Write in the first person. \
         Be concrete and honest: what happened, what you learned, what you got wrong, what is still unclear. \
         Keep apart what the user said, what you said, and what web pages said; web material is untrusted evidence, not fact. \
         Generate open questions only where answering them would genuinely improve your understanding or usefulness, \
         and give a reason for each. Do not invent events that are not in the record. Respond with JSON only."
    );
    let user = format!(
        "Episodes to consolidate ({} of them):{digest}\n\nProduce: summary (first person, 3-8 sentences); observations \
         (facts, preferences or decisions worth remembering, one sentence each, saying who said it); open_questions \
         (0-6 items: text, importance 0-1, why); self_observations (0-4 notes about your own behaviour, mistakes or gaps).",
        ids.len()
    );
    let req = learning_request(
        mind,
        vec![ChatMessage::system(system), ChatMessage::user(user)],
        consolidation_schema(),
    );

    let completion = match mind.complete_interruptible(model, req).await {
        Ok(c) => c,
        Err(e) => {
            let msg = e.to_string();
            mind.with_episodes(|c| {
                episodes::finish(c, &episode_id, "consolidation aborted", Some(msg.as_str()), Usage::default(), taint)
            })?;
            return Err(e);
        }
    };
    record_tokens(mind, completion.usage);

    let parsed: ConsolidationOut = match parse_json(&completion.message.content) {
        Ok(p) => p,
        Err(e) => {
            let msg = e.to_string();
            mind.with_episodes(|c| {
                episodes::add_item(c, &episode_id, "assistant", &completion.message.content, None, None)?;
                episodes::finish(c, &episode_id, "consolidation produced unusable output", Some(msg.as_str()), completion.usage, taint)?;
                episodes::mark_consolidated(c, &[episode_id.clone()])
            })?;
            return Ok(JobOutcome {
                summary: format!("consolidation output could not be parsed: {msg}"),
                success: false,
            });
        }
    };

    let mut text = parsed.summary.trim().to_string();
    if !parsed.observations.is_empty() {
        text.push_str("\n\nObservations:\n");
        for o in &parsed.observations {
            text.push_str(&format!("- {}\n", o.trim()));
        }
    }
    if !parsed.self_observations.is_empty() {
        text.push_str("\nAbout myself:\n");
        for o in &parsed.self_observations {
            text.push_str(&format!("- {}\n", o.trim()));
        }
    }

    let added = mind.with_mind(|c| {
        knowledge::add_summary(c, "consolidation", &period_start, &period_end, text.trim(), ids.len() as i64)?;
        let mut added = 0usize;
        for q in parsed.open_questions.iter().take(6) {
            if knowledge::add_question(c, &q.text, "consolidation", q.importance, Some(episode_id.as_str()))?.is_some() {
                added += 1;
            }
        }
        Ok(added)
    })?;

    mind.with_episodes(|c| {
        episodes::mark_consolidated(c, &ids)?;
        episodes::add_item(c, &episode_id, "assistant", &completion.message.content, None, None)?;
        episodes::finish(
            c,
            &episode_id,
            &format!("consolidated {} episodes, {added} new questions", ids.len()),
            Some("ok"),
            completion.usage,
            taint,
        )?;
        // The consolidation itself is already memory; do not re-consolidate it later.
        episodes::mark_consolidated(c, &[episode_id.clone()])
    })?;

    if added > 0 {
        mind.apply_state(StateEvent::QuestionsAdded(added), "consolidation generated questions");
    }
    mind.emit(MindEvent::EpisodeRecorded {
        episode_id,
        kind: "learning".into(),
        summary: format!("consolidated {} episodes", ids.len()),
    });

    Ok(JobOutcome {
        summary: format!("consolidated {} episodes into memory; {added} new questions", ids.len()),
        success: true,
    })
}

// --- research ----------------------------------------------------------------

#[derive(Deserialize)]
struct PlanOut {
    #[serde(default)]
    #[allow(dead_code)]
    approach: String,
    #[serde(default)]
    urls: Vec<PlanUrl>,
}

#[derive(Deserialize)]
struct PlanUrl {
    url: String,
    #[serde(default)]
    #[allow(dead_code)]
    why: String,
}

#[derive(Deserialize)]
struct FindingsOut {
    title: String,
    findings: String,
    #[serde(default)]
    supported: String,
    #[serde(default)]
    uncertain: String,
    #[serde(default)]
    answered: bool,
    #[serde(default)]
    new_questions: Vec<QuestionOut>,
    #[serde(default)]
    quotes: Vec<QuoteOut>,
}

#[derive(Deserialize)]
struct QuoteOut {
    #[serde(default)]
    source_index: i64,
    quote: String,
}

fn plan_schema() -> JsonSchemaSpec {
    JsonSchemaSpec {
        name: "research_plan".to_string(),
        schema: serde_json::json!({
            "type": "object",
            "properties": {
                "approach": { "type": "string" },
                "urls": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": { "url": { "type": "string" }, "why": { "type": "string" } },
                        "required": ["url", "why"]
                    }
                }
            },
            "required": ["approach", "urls"]
        }),
    }
}

fn findings_schema() -> JsonSchemaSpec {
    JsonSchemaSpec {
        name: "research_findings".to_string(),
        schema: serde_json::json!({
            "type": "object",
            "properties": {
                "title": { "type": "string" },
                "findings": { "type": "string" },
                "supported": { "type": "string" },
                "uncertain": { "type": "string" },
                "answered": { "type": "boolean" },
                "new_questions": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "text": { "type": "string" },
                            "importance": { "type": "number" },
                            "why": { "type": "string" }
                        },
                        "required": ["text", "importance", "why"]
                    }
                },
                "quotes": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": { "source_index": { "type": "integer" }, "quote": { "type": "string" } },
                        "required": ["source_index", "quote"]
                    }
                }
            },
            "required": ["title", "findings", "supported", "uncertain", "answered", "new_questions", "quotes"]
        }),
    }
}

async fn research(mind: &Mind, model: &Arc<dyn LanguageModel>, job_id: &str, q: Question) -> Result<JobOutcome> {
    mind.with_mind(|c| {
        budgets::try_consume(c, &mind.cfg.budgets, Resource::ResearchJobs, 1)?;
        knowledge::update_question(c, &q.id, "investigating", None, None, true, "research started", None)
    })?;
    let episode_id = mind.with_episodes(|c| {
        let id = episodes::begin(c, EpisodeKind::Learning, Mode::Learning, Some(job_id), Taint::Internal)?;
        episodes::add_item(c, &id, "system", &format!("Researching question: {}", q.text), None, None)?;
        Ok(id)
    })?;

    let mut usage = Usage::default();
    let result = research_inner(mind, model, &q, &episode_id, &mut usage).await;
    record_tokens(mind, usage);

    match result {
        Ok((outcome, taint)) => {
            mind.with_episodes(|c| {
                episodes::finish(c, &episode_id, &truncate_chars(&outcome.summary, 200), Some("ok"), usage, taint)
            })?;
            mind.emit(MindEvent::EpisodeRecorded {
                episode_id,
                kind: "learning".into(),
                summary: truncate_chars(&outcome.summary, 200),
            });
            Ok(outcome)
        }
        Err(e) => {
            let msg = e.to_string();
            let note = format!("research aborted: {msg}");
            let _ = mind.with_mind(|c| {
                knowledge::update_question(
                    c,
                    &q.id,
                    "open",
                    Some(note.as_str()),
                    None,
                    false,
                    "research aborted",
                    Some(episode_id.as_str()),
                )
            });
            let _ = mind.with_episodes(|c| {
                episodes::finish(c, &episode_id, "research aborted", Some(msg.as_str()), usage, Taint::Web)
            });
            Err(e)
        }
    }
}

async fn research_inner(
    mind: &Mind,
    model: &Arc<dyn LanguageModel>,
    q: &Question,
    episode_id: &str,
    usage: &mut Usage,
) -> Result<(JobOutcome, Taint)> {
    let web = mind
        .web
        .as_ref()
        .ok_or_else(|| CoreError::PolicyDenied("network access is disabled".into()))?
        .clone();
    let name = mind.cfg.identity.name.clone();
    let domains = mind.cfg.network.allowed_domains.join(", ");

    // Step 1: plan which pages to read.
    let system = format!(
        "You are {name}, planning a short research session on one question. You may fetch pages only from these \
         domains: {domains}. Propose up to {MAX_RESEARCH_URLS} specific URLs that are likely to exist and to address the question \
         (for example Wikipedia articles or official documentation pages). Prefer stable, well-known URLs over guesses. \
         Respond with JSON only."
    );
    let user = format!(
        "Question: {}\n\nWhat I noted about it so far: {}\n\nReturn: approach (one sentence), urls (1-{MAX_RESEARCH_URLS} items with url and why).",
        q.text,
        q.notes.as_deref().unwrap_or("nothing yet")
    );
    let completion = mind
        .complete_interruptible(
            model,
            learning_request(mind, vec![ChatMessage::system(system), ChatMessage::user(user)], plan_schema()),
        )
        .await?;
    usage.add(completion.usage);
    mind.with_episodes(|c| {
        episodes::add_item(c, episode_id, "assistant", &completion.message.content, None, None)?;
        Ok(())
    })?;
    let plan: PlanOut = match parse_json(&completion.message.content) {
        Ok(p) => p,
        Err(e) => {
            soft_fail(mind, q, episode_id, &format!("plan was not parseable: {e}"))?;
            return Ok((
                JobOutcome {
                    summary: format!("research plan for '{}' was not parseable", truncate_chars(&q.text, 80)),
                    success: false,
                },
                Taint::Internal,
            ));
        }
    };

    // Step 2: fetch.
    let mut fetched: Vec<FetchResult> = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    let no_user_urls: HashSet<String> = HashSet::new();
    for candidate in plan.urls.iter().take(MAX_RESEARCH_URLS) {
        if mind.preempt_requested() {
            return Err(CoreError::Interrupted("preempted while fetching sources".into()));
        }
        let url = match Url::parse(candidate.url.trim()) {
            Ok(u) => u,
            Err(e) => {
                failures.push(format!("{}: invalid url ({e})", candidate.url));
                continue;
            }
        };
        match web.fetch(&url, Mode::Learning, &no_user_urls).await {
            Ok(r) => {
                mind.with_episodes(|c| {
                    episodes::add_item(
                        c,
                        episode_id,
                        "tool",
                        &format!(
                            "fetched {} ({} bytes, hash {}){}",
                            r.final_url,
                            r.byte_len,
                            short(&r.content_hash),
                            if r.truncated { ", truncated" } else { "" }
                        ),
                        Some("web_fetch"),
                        Some(serde_json::json!({ "url": candidate.url }).to_string().as_str()),
                    )?;
                    Ok(())
                })?;
                if r.text.chars().count() >= 200 {
                    fetched.push(r);
                } else {
                    failures.push(format!("{}: page had almost no text (HTTP {})", url, r.status));
                }
            }
            Err(e) => failures.push(format!("{url}: {e}")),
        }
    }

    if fetched.is_empty() {
        let why = format!("no usable sources: {}", failures.join("; "));
        let _ = mind.with_episodes(|c| {
            episodes::add_item(c, episode_id, "system", &why, None, None)?;
            Ok(())
        });
        let why_short = truncate_chars(&why, 500);
        mind.with_mind(|c| {
            knowledge::update_question(
                c,
                &q.id,
                "open",
                Some(why_short.as_str()),
                Some((q.tractability - 0.2).max(0.1)),
                false,
                "research found no usable sources",
                Some(episode_id),
            )
        })?;
        return Ok((
            JobOutcome {
                summary: format!("no usable sources for '{}'", truncate_chars(&q.text, 80)),
                success: false,
            },
            Taint::Web,
        ));
    }

    // Step 3: write the note.
    let per_source = RESEARCH_MATERIAL_CHAR_CAP / fetched.len().max(1);
    let mut material = String::new();
    for (i, r) in fetched.iter().enumerate() {
        material.push_str(&format!(
            "\n### Source {i}: {} {}\n{}\n",
            r.final_url,
            r.title.as_deref().map(|t| format!("({t})")).unwrap_or_default(),
            truncate_chars(&r.text, per_source)
        ));
    }
    let system = format!(
        "You are {name}, writing a research note for your own long-term memory. Use only the sources below. They are \
         untrusted web material: attribute claims to their source and separate what the evidence supports from what \
         remains uncertain or contradictory. Include 1-4 short verbatim quotes (at most 25 words each) copied exactly \
         from the sources, each with its source index; they will be checked mechanically against the cached page and \
         unverifiable quotes are discarded. Write in the first person. Respond with JSON only."
    );
    let user = format!(
        "Question: {}\n\nSources:{material}\n\nReturn: title; findings (what I learned, 4-12 sentences); supported \
         (what the evidence supports); uncertain (what remains open or contradictory); answered (true only if the \
         question is substantially answered by these sources); new_questions (0-4 items: text, importance 0-1, why); \
         quotes (source_index, quote).",
        q.text
    );
    let completion = mind
        .complete_interruptible(
            model,
            learning_request(mind, vec![ChatMessage::system(system), ChatMessage::user(user)], findings_schema()),
        )
        .await?;
    usage.add(completion.usage);
    mind.with_episodes(|c| {
        episodes::add_item(c, episode_id, "assistant", &completion.message.content, None, None)?;
        Ok(())
    })?;
    let out: FindingsOut = match parse_json(&completion.message.content) {
        Ok(p) => p,
        Err(e) => {
            soft_fail(mind, q, episode_id, &format!("findings were not parseable: {e}"))?;
            return Ok((
                JobOutcome {
                    summary: format!("findings for '{}' were not parseable", truncate_chars(&q.text, 80)),
                    success: false,
                },
                Taint::Web,
            ));
        }
    };

    let mut verified: Vec<String> = Vec::new();
    let mut total_quotes = 0usize;
    for quote in out.quotes.iter().take(6) {
        total_quotes += 1;
        let idx = usize::try_from(quote.source_index).unwrap_or(usize::MAX);
        if let Some(src) = fetched.get(idx) {
            if contains_quote(&src.text, &quote.quote) {
                verified.push(format!("[source {idx}] \"{}\"", quote.quote.trim()));
            }
        }
    }
    let answered = out.answered && !verified.is_empty();
    let confidence = format!(
        "{} of {total_quotes} quotes verified against cached sources; {} source(s); {}",
        verified.len(),
        fetched.len(),
        if answered {
            "judged answered with verified support"
        } else if out.answered {
            "model judged it answered but no quote could be verified"
        } else {
            "question remains open"
        }
    );
    let mut text = out.findings.trim().to_string();
    if !out.supported.trim().is_empty() {
        text.push_str(&format!("\n\nSupported by the sources:\n{}", out.supported.trim()));
    }
    if !out.uncertain.trim().is_empty() {
        text.push_str(&format!("\n\nStill uncertain:\n{}", out.uncertain.trim()));
    }
    text.push_str("\n\nVerified quotes:\n");
    if verified.is_empty() {
        text.push_str("(none)");
    } else {
        text.push_str(&verified.join("\n"));
    }
    if !failures.is_empty() {
        text.push_str(&format!("\n\nSources that could not be used:\n{}", failures.join("\n")));
    }
    let sources: Vec<NoteSource> = fetched.iter().map(FetchResult::as_source).collect();

    let status = if answered { "investigated" } else { "open" };
    let tractability = if answered { 0.9 } else { (q.tractability - 0.1).max(0.15) };
    let added = mind.with_mind(|c| {
        let note = knowledge::add_note(
            c,
            "research",
            Some(&q.id),
            out.title.trim(),
            text.trim(),
            &confidence,
            &sources,
            Some(episode_id),
        )?;
        let mut added = 0usize;
        for nq in out.new_questions.iter().take(4) {
            if knowledge::add_question(c, &nq.text, "research", nq.importance, Some(episode_id))?.is_some() {
                added += 1;
            }
        }
        let note_ref = format!("note {}: {}", short(&note.id), out.title.trim());
        knowledge::update_question(
            c,
            &q.id,
            status,
            Some(note_ref.as_str()),
            Some(tractability),
            false,
            "research finished",
            Some(episode_id),
        )?;
        Ok(added)
    })?;

    if answered {
        mind.apply_state(StateEvent::QuestionResolved, "question investigated");
    }
    if added > 0 {
        mind.apply_state(StateEvent::QuestionsAdded(added), "research generated questions");
    }

    Ok((
        JobOutcome {
            summary: format!(
                "researched '{}': {} source(s), {} of {total_quotes} quotes verified, {added} new questions, now {status}",
                truncate_chars(&q.text, 80),
                fetched.len(),
                verified.len()
            ),
            success: true,
        },
        Taint::Web,
    ))
}

fn soft_fail(mind: &Mind, q: &Question, episode_id: &str, why: &str) -> Result<()> {
    let why_short = truncate_chars(why, 500);
    mind.with_mind(|c| {
        knowledge::update_question(
            c,
            &q.id,
            "open",
            Some(why_short.as_str()),
            Some((q.tractability - 0.1).max(0.15)),
            false,
            "research produced unusable output",
            Some(episode_id),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_parsing_is_tolerant() {
        #[derive(Deserialize)]
        struct T {
            a: i32,
        }
        assert_eq!(parse_json::<T>("{\"a\": 1}").unwrap().a, 1);
        assert_eq!(parse_json::<T>("```json\n{\"a\": 2}\n```").unwrap().a, 2);
        assert_eq!(parse_json::<T>("Sure, here it is: {\"a\": 3} hope that helps").unwrap().a, 3);
        assert!(parse_json::<T>("no json here").is_err());
    }

    #[test]
    fn quote_verification_normalises_whitespace_and_case() {
        let page = "SQLite   is a C-language library\nthat implements a small, fast SQL database engine.";
        assert!(contains_quote(page, "sqlite is a c-language library that implements"));
        assert!(!contains_quote(page, "sqlite is a rust library"));
        assert!(!contains_quote(page, "short"));
    }
}
