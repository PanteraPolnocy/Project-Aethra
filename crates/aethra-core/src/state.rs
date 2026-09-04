//! Internal state: a handful of bounded scalars that shape scheduling and
//! prompt conditioning. Updated only by the deterministic rules below, so the
//! numbers mean something and cannot be talked into a different value.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::util::{clamp01, now_rfc3339};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct InternalState {
    /// Appetite for open questions and new material. Raised by unanswered
    /// questions, lowered by a day of sleep towards baseline.
    pub curiosity: f64,
    /// Tendency to stay on the current thread. Raised by conversation,
    /// decays daily.
    pub focus: f64,
    /// Remaining appetite for work today. Spent by jobs, restored each day.
    pub energy: f64,
    /// Trust in own recent performance. Moves with grounded outcomes only.
    pub confidence: f64,
}

impl Default for InternalState {
    fn default() -> Self {
        Self {
            curiosity: 0.5,
            focus: 0.5,
            energy: 1.0,
            confidence: 0.5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StateEvent {
    ChatTurn,
    LearningJobFinished { success: bool },
    QuestionsAdded(usize),
    QuestionResolved,
    DayStarted,
    ModelUnavailable,
}

impl InternalState {
    pub fn apply(&mut self, ev: StateEvent) {
        match ev {
            StateEvent::ChatTurn => {
                // self.energy -= 0.01;
                self.focus += 0.03;
            }
            StateEvent::LearningJobFinished { success } => {
                // self.energy -= 0.05;
                if success {
                    self.confidence += 0.02;
                } else {
                    self.confidence -= 0.03;
                }
            }
            StateEvent::QuestionsAdded(n) => {
                self.curiosity += 0.03 * n.min(10) as f64;
            }
            StateEvent::QuestionResolved => {
                self.curiosity -= 0.02;
                self.confidence += 0.01;
            }
            StateEvent::DayStarted => {
                self.energy = 1.0;
                self.curiosity += (0.5 - self.curiosity) * 0.2;
                self.focus += (0.5 - self.focus) * 0.5;
            }
            StateEvent::ModelUnavailable => {
                self.confidence -= 0.05;
            }
        }
        self.energy = clamp01(self.energy).max(0.05);
        self.curiosity = clamp01(self.curiosity);
        self.focus = clamp01(self.focus);
        self.confidence = clamp01(self.confidence);
    }

    pub fn describe(&self) -> String {
        format!(
            "curiosity {:.2}, focus {:.2}, energy {:.2}, confidence {:.2}",
            self.curiosity, self.focus, self.energy, self.confidence
        )
    }
}

pub fn load(conn: &Connection) -> Result<InternalState> {
    let mut state = InternalState::default();
    let mut stmt = conn.prepare("SELECT name, value FROM internal_state")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?)))?;
    for row in rows {
        let (name, value) = row?;
        match name.as_str() {
            "curiosity" => state.curiosity = clamp01(value),
            "focus" => state.focus = clamp01(value),
            "energy" => state.energy = clamp01(value),
            "confidence" => state.confidence = clamp01(value),
            _ => {}
        }
    }
    Ok(state)
}

pub fn save(conn: &Connection, state: &InternalState, reason: &str) -> Result<()> {
    let now = now_rfc3339();
    for (name, value) in [
        ("curiosity", state.curiosity),
        ("focus", state.focus),
        ("energy", state.energy),
        ("confidence", state.confidence),
    ] {
        conn.execute(
            "INSERT INTO internal_state (name, value, updated_at, reason) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(name) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at, reason = excluded.reason",
            params![name, value, now, reason],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_stay_bounded() {
        let mut s = InternalState::default();
        for _ in 0..500 {
            s.apply(StateEvent::QuestionsAdded(10));
            s.apply(StateEvent::LearningJobFinished { success: false });
        }
        assert!(s.curiosity <= 1.0);
        assert!(s.energy >= 0.05);
        assert!(s.confidence >= 0.0);
        s.apply(StateEvent::DayStarted);
        assert_eq!(s.energy, 1.0);
    }
}
