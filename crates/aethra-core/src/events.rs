//! Events the mind broadcasts to whoever is watching (the desktop shell, a CLI).

use serde::Serialize;

use crate::mode::{LearningGate, Mode};
use crate::state::InternalState;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MindEvent {
    ModeChanged { mode: Mode },
    LearningGateChanged { gate: LearningGate },
    EpisodeRecorded { episode_id: String, kind: String, summary: String },
    JobStarted { job_id: String, kind: String, detail: String },
    JobFinished { job_id: String, kind: String, outcome: String, success: bool },
    ModelStatus { reachable: bool, loaded: bool, detail: String },
    StateChanged { state: InternalState },
    Log { level: String, message: String },
}
