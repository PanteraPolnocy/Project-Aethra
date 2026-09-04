//! The mind: one struct that owns storage, the model host, tools and mode
//! state, and exposes the operations the shell needs. UI-agnostic.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use aethra_models::{ChatMessage, Completion, CompletionRequest, LanguageModel, ModelError, Usage};
use chrono::{DateTime, Local, Utc};
use parking_lot::{Mutex, RwLock};
use rusqlite::Connection;
use serde::Serialize;
use tokio::sync::{broadcast, Notify};

use crate::budgets::{self, BudgetStatus, Resource};
use crate::changes::{self, ChangeRow};
use crate::config::AppConfig;
use crate::context::{build_system_prompt, ContextInputs};
use crate::db::Databases;
use crate::episodes::{self, EpisodeItem, EpisodeKind, EpisodeRow, Taint};
use crate::error::{CoreError, Result};
use crate::events::MindEvent;
use crate::identity::{self, Constitution, SelfModelSection};
use crate::jobs::{self, JobOutcome};
use crate::knowledge::{self, Note, Question, Summary};
use crate::mode::{evaluate_gate, LearningGate, Mode};
use crate::model_host::{ModelHost, Profile};
use crate::policy;
use crate::state::{self, InternalState, StateEvent};
use crate::tools::web_fetch::WebFetchTool;
use crate::tools::{ToolContext, ToolRegistry};
use crate::util::{new_id, to_rfc3339, today_local, truncate_chars};

pub struct Mind {
    pub cfg: Arc<AppConfig>,
    pub config_path: PathBuf,
    pub dbs: Arc<Databases>,
    pub(crate) host: ModelHost,
    pub(crate) tools: ToolRegistry,
    pub(crate) web: Option<Arc<WebFetchTool>>,
    mode: RwLock<Mode>,
    last_user_activity: Mutex<DateTime<Utc>>,
    learning_requested: AtomicBool,
    learning_stopped: AtomicBool,
    preempt_flag: AtomicBool,
    preempt: Notify,
    /// Held for the duration of any model-driven activity (a chat turn or a job).
    activity: tokio::sync::Mutex<()>,
    current_job: Mutex<Option<String>>,
    last_gate: Mutex<Option<LearningGate>>,
    last_day: Mutex<String>,
    events: broadcast::Sender<MindEvent>,
    started_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MindStatus {
    pub name: String,
    pub mode: Mode,
    pub learning_gate: LearningGate,
    pub learning_requested: bool,
    pub model_reachable: bool,
    pub model_loaded_profile: Option<String>,
    pub sidecar_managed: bool,
    pub state: InternalState,
    pub budgets: Vec<BudgetStatus>,
    pub open_questions: i64,
    pub unconsolidated_episodes: i64,
    pub total_episodes: i64,
    pub notes: i64,
    pub current_job: Option<String>,
    pub last_user_activity: String,
    pub uptime_secs: i64,
    pub data_dir: String,
    pub config_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatReply {
    pub text: String,
    pub episode_id: String,
    pub tool_uses: Vec<String>,
    pub usage: Usage,
    pub taint: Taint,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TickOutcome {
    Waiting { gate: LearningGate },
    Busy,
    NothingToDo,
    Ran { job: String, outcome: String, success: bool },
    Preempted { job: String },
}

impl Mind {
    pub async fn open(cfg: AppConfig, config_path: PathBuf) -> Result<Arc<Self>> {
        cfg.validate()?;
        let cfg = Arc::new(cfg);
        std::fs::create_dir_all(cfg.workspace_dir())?;
        let dbs = Arc::new(Databases::open(&cfg)?);
        {
            let mind = dbs.mind.lock();
            identity::ensure_defaults(&mind)?;
        }
        let host = ModelHost::new(cfg.clone())?;

        let mut tools = ToolRegistry::new();
        let web = if cfg.network.enabled {
            let tool = Arc::new(WebFetchTool::new(cfg.clone(), dbs.clone())?);
            tools.register(tool.clone());
            Some(tool)
        } else {
            None
        };

        let (events, _) = broadcast::channel(256);
        let mind = Arc::new(Self {
            cfg,
            config_path,
            dbs,
            host,
            tools,
            web,
            mode: RwLock::new(Mode::Idle),
            last_user_activity: Mutex::new(Utc::now()),
            learning_requested: AtomicBool::new(false),
            learning_stopped: AtomicBool::new(false),
            preempt_flag: AtomicBool::new(false),
            preempt: Notify::new(),
            activity: tokio::sync::Mutex::new(()),
            current_job: Mutex::new(None),
            last_gate: Mutex::new(None),
            last_day: Mutex::new(today_local()),
            events,
            started_at: Utc::now(),
        });
        mind.record_system_episode("mind started").ok();
        Ok(mind)
    }

    // --- events and mode ----------------------------------------------------

    pub fn subscribe(&self) -> broadcast::Receiver<MindEvent> {
        self.events.subscribe()
    }

    pub(crate) fn emit(&self, ev: MindEvent) {
        let _ = self.events.send(ev);
    }

    pub fn mode(&self) -> Mode {
        *self.mode.read()
    }

    pub(crate) fn set_mode(&self, mode: Mode) {
        let changed = {
            let mut m = self.mode.write();
            if *m == mode {
                false
            } else {
                *m = mode;
                true
            }
        };
        if changed {
            tracing::info!(mode = mode.as_str(), "mode changed");
            self.emit(MindEvent::ModeChanged { mode });
        }
    }

    pub fn touch_user_activity(&self) {
        *self.last_user_activity.lock() = Utc::now();
    }

    pub fn request_learning(&self) {
        self.learning_stopped.store(false, Ordering::SeqCst);
        self.learning_requested.store(true, Ordering::SeqCst);
        self.emit(MindEvent::Log {
            level: "info".into(),
            message: "learning requested".into(),
        });
    }

    pub fn stop_learning(&self) {
        self.learning_requested.store(false, Ordering::SeqCst);
        self.learning_stopped.store(true, Ordering::SeqCst);
        self.preempt_flag.store(true, Ordering::SeqCst);
        self.preempt.notify_waiters();
        self.emit(MindEvent::Log {
            level: "info".into(),
            message: "learning stopped by the user".into(),
        });
    }

    /// Manual stop is cleared by the next learning window or a new request.
    pub fn learning_gate(&self) -> LearningGate {
        evaluate_gate(
            &self.cfg.modes,
            Local::now(),
            *self.last_user_activity.lock(),
            self.learning_requested.load(Ordering::SeqCst),
            self.learning_stopped.load(Ordering::SeqCst),
        )
    }

    pub(crate) fn preempt_requested(&self) -> bool {
        self.preempt_flag.load(Ordering::SeqCst)
    }

    // --- storage helpers ----------------------------------------------------

    pub(crate) fn with_mind<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let conn = self.dbs.mind.lock();
        f(&conn)
    }

    pub(crate) fn with_episodes<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let conn = self.dbs.episodes.lock();
        f(&conn)
    }

    pub(crate) fn apply_state(&self, ev: StateEvent, reason: &str) {
        let result = self.with_mind(|c| {
            let mut s = state::load(c)?;
            s.apply(ev);
            state::save(c, &s, reason)?;
            Ok(s)
        });
        match result {
            Ok(s) => self.emit(MindEvent::StateChanged { state: s }),
            Err(e) => tracing::warn!("state update failed: {e}"),
        }
    }

    fn record_system_episode(&self, summary: &str) -> Result<()> {
        self.with_episodes(|c| {
            let id = episodes::begin(c, EpisodeKind::System, Mode::Idle, None, Taint::Internal)?;
            episodes::finish(c, &id, summary, Some("ok"), Usage::default(), Taint::Internal)?;
            Ok(())
        })
    }

    // --- read API for the shell --------------------------------------------

    pub async fn status(&self) -> Result<MindStatus> {
        let (state, budgets, open_questions, notes) = self.with_mind(|c| {
            Ok((
                state::load(c)?,
                budgets::snapshot(c, &self.cfg.budgets)?,
                knowledge::count_open(c)?,
                knowledge::count_notes(c)?,
            ))
        })?;
        let (unconsolidated, total) =
            self.with_episodes(|c| Ok((episodes::count_unconsolidated(c)?, episodes::count_all(c)?)))?;
        let loaded = self.host.loaded_profile().await.map(|p| p.as_str().to_string());
        let reachable = self.host.reachable().await;
        Ok(MindStatus {
            name: self.cfg.identity.name.clone(),
            mode: self.mode(),
            learning_gate: self.learning_gate(),
            learning_requested: self.learning_requested.load(Ordering::SeqCst),
            model_reachable: reachable,
            model_loaded_profile: loaded,
            sidecar_managed: self.cfg.models.sidecar.enabled,
            state,
            budgets,
            open_questions,
            unconsolidated_episodes: unconsolidated,
            total_episodes: total,
            notes,
            current_job: self.current_job.lock().clone(),
            last_user_activity: to_rfc3339(*self.last_user_activity.lock()),
            uptime_secs: (Utc::now() - self.started_at).num_seconds(),
            data_dir: self.cfg.data_dir.display().to_string(),
            config_path: self.config_path.display().to_string(),
        })
    }

    pub fn timeline(&self, limit: u32, before: Option<&str>) -> Result<Vec<EpisodeRow>> {
        self.with_episodes(|c| episodes::list(c, limit.clamp(1, 500), before))
    }

    pub fn episode_items(&self, episode_id: &str) -> Result<Vec<EpisodeItem>> {
        self.with_episodes(|c| episodes::items(c, episode_id))
    }

    pub fn questions(&self, status: Option<&str>, limit: u32) -> Result<Vec<Question>> {
        self.with_mind(|c| knowledge::list_questions(c, status, limit.clamp(1, 500)))
    }

    pub fn add_user_question(&self, text: &str) -> Result<Option<Question>> {
        self.touch_user_activity();
        let q = self.with_mind(|c| knowledge::add_question(c, text, "user", 0.8, None))?;
        if q.is_some() {
            self.apply_state(StateEvent::QuestionsAdded(1), "user added a question");
        }
        Ok(q)
    }

    pub fn retire_question(&self, id: &str) -> Result<()> {
        self.touch_user_activity();
        self.with_mind(|c| {
            knowledge::update_question(c, id, "retired", Some("retired by the user"), None, false, "retired by the user", None)
        })
    }

    pub fn notes(&self, limit: u32) -> Result<Vec<Note>> {
        self.with_mind(|c| knowledge::list_notes(c, limit.clamp(1, 500)))
    }

    pub fn summaries(&self, limit: u32) -> Result<Vec<Summary>> {
        self.with_mind(|c| knowledge::list_summaries(c, None, limit.clamp(1, 500)))
    }

    pub fn self_model(&self) -> Result<Vec<SelfModelSection>> {
        self.with_mind(identity::get_self_model)
    }

    pub fn constitution(&self) -> Result<Constitution> {
        self.with_mind(identity::get_constitution)
    }

    pub fn set_constitution(&self, text: &str) -> Result<Constitution> {
        self.touch_user_activity();
        self.with_mind(|c| identity::set_constitution(c, text))
    }

    pub fn changes(&self, limit: u32) -> Result<Vec<ChangeRow>> {
        self.with_mind(|c| changes::list_recent(c, limit.clamp(1, 1000)))
    }

    pub fn snapshot(&self) -> Result<Vec<PathBuf>> {
        let dir = self.cfg.data_dir.join("snapshots");
        self.dbs.snapshot(&dir)
    }

    // --- model access with preemption --------------------------------------

    /// Runs a completion but abandons it if the user preempts (a chat message
    /// or a manual stop). Dropping the request closes the HTTP connection,
    /// which llama-server treats as cancellation.
    pub(crate) async fn complete_interruptible(
        &self,
        model: &Arc<dyn LanguageModel>,
        req: CompletionRequest,
    ) -> Result<Completion> {
        if self.preempt_requested() {
            return Err(CoreError::Interrupted("preempted before the model call".into()));
        }
        tokio::select! {
            r = model.complete(req) => r.map_err(CoreError::from),
            _ = self.preempt.notified() => Err(CoreError::Interrupted("preempted during the model call".into())),
        }
    }

    // --- chat ----------------------------------------------------------------

    pub async fn chat(&self, text: &str) -> Result<ChatReply> {
        let text = text.trim();
        if text.is_empty() {
            return Err(CoreError::other("empty message"));
        }
        self.touch_user_activity();

        // Ask any running job to yield, then take the activity lock.
        self.preempt_flag.store(true, Ordering::SeqCst);
        self.preempt.notify_waiters();
        let _activity = self.activity.lock().await;
        self.preempt_flag.store(false, Ordering::SeqCst);
        self.set_mode(Mode::Chat);

        let model = match self.host.ensure(Profile::Chat).await {
            Ok(m) => m,
            Err(e) => {
                self.apply_state(StateEvent::ModelUnavailable, "model unavailable for chat");
                self.emit(MindEvent::ModelStatus {
                    reachable: false,
                    loaded: false,
                    detail: e.to_string(),
                });
                return Err(e);
            }
        };
        let profile = self.host.profile_config(Profile::Chat).clone();

        let system_prompt = self.build_prompt(Mode::Chat)?;
        let history = self.with_episodes(|c| {
            episodes::recent_conversation_items(c, self.cfg.context.max_history_turns)
        })?;
        let mut messages: Vec<ChatMessage> = Vec::with_capacity(history.len() + 2);
        messages.push(ChatMessage::system(system_prompt));
        for item in history {
            match item.role.as_str() {
                "user" => messages.push(ChatMessage::user(item.content)),
                "assistant" => messages.push(ChatMessage::assistant(item.content)),
                _ => {}
            }
        }
        messages.push(ChatMessage::user(text));

        let episode_id = self.with_episodes(|c| {
            let id = episodes::begin(c, EpisodeKind::Conversation, Mode::Chat, None, Taint::User)?;
            episodes::add_item(c, &id, "user", text, None, None)?;
            Ok(id)
        })?;

        let tool_specs = if self.tools.is_empty() { Vec::new() } else { self.tools.specs() };
        let ctx = ToolContext {
            mode: Mode::Chat,
            user_urls: policy::user_url_set(text),
            episode_id: episode_id.clone(),
        };

        let mut usage = Usage::default();
        let mut taint = Taint::User;
        let mut tool_uses = Vec::new();
        let mut reply = String::new();
        let max_rounds = self.cfg.context.max_tool_rounds;

        for round in 0..=max_rounds {
            let req = CompletionRequest {
                messages: messages.clone(),
                tools: if round < max_rounds { tool_specs.clone() } else { Vec::new() },
                json_schema: None,
                max_tokens: Some(profile.max_tokens),
                temperature: Some(profile.temperature as f32),
            };
            let completion = match model.complete(req).await {
                Ok(c) => c,
                Err(e) => {
                    let detail = format!("model error: {e}");
                    self.with_episodes(|c| {
                        episodes::finish(c, &episode_id, &truncate_chars(text, 120), Some(detail.as_str()), usage, taint)
                    })?;
                    if matches!(e, ModelError::Unreachable(_) | ModelError::Timeout) {
                        self.apply_state(StateEvent::ModelUnavailable, "model failed mid-conversation");
                    }
                    return Err(e.into());
                }
            };
            usage.add(completion.usage);

            if completion.message.tool_calls.is_empty() {
                reply = completion.message.content.trim().to_string();
                break;
            }

            messages.push(completion.message.clone());
            for call in &completion.message.tool_calls {
                let args_text = call.arguments.to_string();
                let result_text = match self.tools.get(&call.name) {
                    Some(tool) => match tool.call(call.arguments.clone(), &ctx).await {
                        Ok(out) => {
                            taint = taint.escalate(out.taint);
                            out.content
                        }
                        Err(e) => format!("TOOL ERROR: {e}"),
                    },
                    None => format!("TOOL ERROR: unknown tool '{}'", call.name),
                };
                tool_uses.push(format!("{}({})", call.name, truncate_chars(&args_text, 200)));
                self.with_episodes(|c| {
                    episodes::add_item(
                        c,
                        &episode_id,
                        "tool",
                        &truncate_chars(&result_text, 4000),
                        Some(call.name.as_str()),
                        Some(args_text.as_str()),
                    )?;
                    Ok(())
                })?;
                messages.push(ChatMessage::tool_result(call.id.clone(), result_text));
            }
        }

        if reply.is_empty() {
            reply = "I could not produce an answer within my tool budget for this turn. Ask again and I will try a more direct route.".to_string();
        }

        self.with_episodes(|c| {
            episodes::add_item(c, &episode_id, "assistant", &reply, None, None)?;
            episodes::finish(c, &episode_id, &truncate_chars(text, 120), Some("ok"), usage, taint)?;
            Ok(())
        })?;
        self.apply_state(StateEvent::ChatTurn, "chat turn");
        self.emit(MindEvent::EpisodeRecorded {
            episode_id: episode_id.clone(),
            kind: "conversation".into(),
            summary: truncate_chars(text, 120),
        });
        self.host.touch();
        self.touch_user_activity();

        Ok(ChatReply {
            text: reply,
            episode_id,
            tool_uses,
            usage,
            taint,
        })
    }

    pub(crate) fn build_prompt(&self, mode: Mode) -> Result<String> {
        let (constitution, self_model, state, summaries, notes, questions) = self.with_mind(|c| {
            Ok((
                identity::get_constitution(c)?,
                identity::get_self_model(c)?,
                state::load(c)?,
                knowledge::list_summaries(c, None, 5)?,
                knowledge::list_notes(c, 10)?,
                knowledge::list_questions(c, Some("open"), 6)?,
            ))
        })?;
        let tool_names = self.tools.names();
        let now_local = Local::now().format("%A %Y-%m-%d %H:%M").to_string();
        Ok(build_system_prompt(
            &self.cfg.context,
            &ContextInputs {
                name: &self.cfg.identity.name,
                mode,
                now_local: &now_local,
                constitution: &constitution.text,
                self_model: &self_model,
                state: &state,
                summaries: &summaries,
                notes: &notes,
                open_questions: &questions,
                tool_names: &tool_names,
                allowed_domains: &self.cfg.network.allowed_domains,
                network_enabled: self.cfg.network.enabled,
            },
        ))
    }

    // --- scheduler entry point ---------------------------------------------

    /// One scheduler step. Cheap when there is nothing to do.
    pub async fn tick(&self) -> Result<TickOutcome> {
        self.day_rollover();

        let gate = self.learning_gate();
        self.publish_gate(&gate);

        if !gate.is_allowed() {
            self.settle_idle().await;
            return Ok(TickOutcome::Waiting { gate });
        }
        if self.preempt_requested() {
            return Ok(TickOutcome::Busy);
        }
        let Ok(_activity) = self.activity.try_lock() else {
            return Ok(TickOutcome::Busy);
        };

        let (energy, tokens_ok, minutes_ok) = self.with_mind(|c| {
            Ok((
                state::load(c)?.energy,
                budgets::has_headroom(c, &self.cfg.budgets, Resource::LearningTokens, 2_000)?,
                budgets::has_headroom(c, &self.cfg.budgets, Resource::LearningMinutes, 1)?,
            ))
        })?;
        if /* energy < 0.1 || */ !tokens_ok || !minutes_ok {
            let reason = /* if energy < 0.1 {
                "energy depleted for today"
            } else */ if !tokens_ok {
                "learning token budget spent"
            } else {
                "learning time budget spent"
            };
            self.learning_requested.store(false, Ordering::SeqCst);
            self.set_mode(Mode::Idle);
            let gate = LearningGate::BudgetExhausted {
                reason: reason.to_string(),
            };
            self.publish_gate(&gate);
            return Ok(TickOutcome::Waiting { gate });
        }

        let Some(job) = jobs::pick_job(self)? else {
            self.learning_requested.store(false, Ordering::SeqCst);
            self.set_mode(Mode::Idle);
            return Ok(TickOutcome::NothingToDo);
        };

        self.set_mode(Mode::Learning);
        let job_id = new_id();
        let kind = job.kind_str().to_string();
        let detail = job.detail();
        *self.current_job.lock() = Some(format!("{kind}: {detail}"));
        self.with_mind(|c| jobs::insert_job(c, &job_id, &job))?;
        self.emit(MindEvent::JobStarted {
            job_id: job_id.clone(),
            kind: kind.clone(),
            detail: detail.clone(),
        });
        tracing::info!(job = %kind, %detail, "job started");

        let started = std::time::Instant::now();
        let result = jobs::run_job(self, &job_id, job).await;
        let elapsed_min = ((started.elapsed().as_secs() + 59) / 60).max(1);
        self.with_mind(|c| budgets::consume_unchecked(c, Resource::LearningMinutes, elapsed_min))?;
        *self.current_job.lock() = None;

        let outcome = match result {
            Ok(JobOutcome { summary, success }) => {
                self.with_mind(|c| {
                    jobs::finish_job(c, &job_id, if success { "done" } else { "failed" }, Some(summary.as_str()), None)
                })?;
                self.apply_state(StateEvent::LearningJobFinished { success }, "learning job finished");
                self.emit(MindEvent::JobFinished {
                    job_id,
                    kind: kind.clone(),
                    outcome: summary.clone(),
                    success,
                });
                TickOutcome::Ran {
                    job: kind,
                    outcome: summary,
                    success,
                }
            }
            Err(CoreError::Interrupted(why)) => {
                self.with_mind(|c| jobs::finish_job(c, &job_id, "preempted", None, Some(why.as_str())))?;
                self.emit(MindEvent::JobFinished {
                    job_id,
                    kind: kind.clone(),
                    outcome: format!("preempted: {why}"),
                    success: false,
                });
                TickOutcome::Preempted { job: kind }
            }
            Err(e) => {
                let msg = e.to_string();
                self.with_mind(|c| jobs::finish_job(c, &job_id, "failed", None, Some(msg.as_str())))?;
                self.apply_state(StateEvent::LearningJobFinished { success: false }, "learning job failed");
                if matches!(e, CoreError::Model(ModelError::Unreachable(_)) | CoreError::Model(ModelError::Timeout)) {
                    self.apply_state(StateEvent::ModelUnavailable, "model unavailable during learning");
                }
                self.emit(MindEvent::JobFinished {
                    job_id,
                    kind: kind.clone(),
                    outcome: msg.clone(),
                    success: false,
                });
                tracing::warn!(job = %kind, "job failed: {msg}");
                TickOutcome::Ran {
                    job: kind,
                    outcome: msg,
                    success: false,
                }
            }
        };
        Ok(outcome)
    }

    fn publish_gate(&self, gate: &LearningGate) {
        let mut last = self.last_gate.lock();
        if last.as_ref() != Some(gate) {
            *last = Some(gate.clone());
            self.emit(MindEvent::LearningGateChanged { gate: gate.clone() });
        }
    }

    /// Outside the learning window: drop back to Idle after the user has been
    /// quiet, and unload the model after the configured grace period.
    async fn settle_idle(&self) {
        let quiet = (Utc::now() - *self.last_user_activity.lock()).num_seconds();
        let quiet_limit = i64::from(self.cfg.modes.quiet_minutes_before_learning) * 60;
        if self.mode() == Mode::Learning || (self.mode() == Mode::Chat && quiet >= quiet_limit) {
            self.set_mode(Mode::Idle);
        }
        if self.mode() == Mode::Idle && self.cfg.models.sidecar.enabled {
            let grace = Duration::from_secs(u64::from(self.cfg.modes.idle_unload_after_minutes) * 60);
            if self.host.unload_if_idle(grace).await {
                self.emit(MindEvent::ModelStatus {
                    reachable: false,
                    loaded: false,
                    detail: "inference server unloaded after idle period".into(),
                });
            }
        }
        // A manual stop only lasts until the user is quiet again for the window's
        // quiet period; otherwise a stop at 01:00 would silence every future night.
        if self.learning_stopped.load(Ordering::SeqCst) && quiet >= quiet_limit.max(1) * 3 {
            self.learning_stopped.store(false, Ordering::SeqCst);
        }
    }

    fn day_rollover(&self) {
        let today = today_local();
        let mut last = self.last_day.lock();
        if *last != today {
            *last = today;
            drop(last);
            self.apply_state(StateEvent::DayStarted, "new local day");
            let _ = self.record_system_episode("new day");
        }
    }

    pub async fn shutdown(&self) {
        self.stop_learning();
        let _ = self.record_system_episode("mind shutting down");
        self.host.shutdown().await;
    }
}
