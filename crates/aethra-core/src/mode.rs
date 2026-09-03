//! Operating modes and the rules that move between them.
//!
//! - Chat:     the user is present; the model answers; tools obey chat policy.
//! - Learning: autonomous jobs run inside the configured window, or on request.
//! - Idle:     nothing scheduled; the inference server may be unloaded.

use chrono::{DateTime, Local, Timelike, Utc};
use serde::{Deserialize, Serialize};

use crate::config::{parse_hhmm, ModesConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Idle,
    Chat,
    Learning,
}

impl Mode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Mode::Idle => "idle",
            Mode::Chat => "chat",
            Mode::Learning => "learning",
        }
    }
}

/// Why learning is or is not permitted right now. Shown in the UI so the
/// user can see what the scheduler is waiting for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LearningGate {
    Allowed { reason: String },
    OutsideWindow { window: String },
    UserActive { quiet_for_secs: i64, required_secs: i64 },
    BudgetExhausted { reason: String },
    ManuallyStopped,
}

impl LearningGate {
    pub fn is_allowed(&self) -> bool {
        matches!(self, LearningGate::Allowed { .. })
    }
}

pub fn minutes_since_midnight(t: DateTime<Local>) -> u32 {
    t.hour() * 60 + t.minute()
}

/// True when `now_min` (minutes since local midnight) lies inside [start, end),
/// handling windows that wrap past midnight.
pub fn in_window(now_min: u32, start_min: u32, end_min: u32) -> bool {
    if start_min == end_min {
        return true;
    }
    if start_min < end_min {
        now_min >= start_min && now_min < end_min
    } else {
        now_min >= start_min || now_min < end_min
    }
}

pub fn evaluate_gate(
    cfg: &ModesConfig,
    now_local: DateTime<Local>,
    last_user_activity: DateTime<Utc>,
    manual_request: bool,
    manual_stop: bool,
) -> LearningGate {
    if manual_stop {
        return LearningGate::ManuallyStopped;
    }
    let quiet_for = (Utc::now() - last_user_activity).num_seconds().max(0);
    if manual_request {
        return LearningGate::Allowed {
            reason: "requested by the user".to_string(),
        };
    }
    let start = parse_hhmm(&cfg.learning_window_start).unwrap_or(60);
    let end = parse_hhmm(&cfg.learning_window_end).unwrap_or(7 * 60);
    if !in_window(minutes_since_midnight(now_local), start, end) {
        return LearningGate::OutsideWindow {
            window: format!("{} - {}", cfg.learning_window_start, cfg.learning_window_end),
        };
    }
    let required = i64::from(cfg.quiet_minutes_before_learning) * 60;
    if quiet_for < required {
        return LearningGate::UserActive {
            quiet_for_secs: quiet_for,
            required_secs: required,
        };
    }
    LearningGate::Allowed {
        reason: "inside learning window and the user has been quiet".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_wraps_midnight() {
        let start = 23 * 60;
        let end = 6 * 60;
        assert!(in_window(23 * 60 + 30, start, end));
        assert!(in_window(2 * 60, start, end));
        assert!(!in_window(12 * 60, start, end));
        assert!(in_window(3 * 60, 60, 7 * 60));
        assert!(!in_window(8 * 60, 60, 7 * 60));
    }
}
