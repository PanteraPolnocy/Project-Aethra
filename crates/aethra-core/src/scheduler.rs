//! The heartbeat. Wakes on a timer, asks the mind to take one step, and
//! sleeps again. All policy lives in `Mind::tick`; this loop only paces it.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;

use crate::mind::{Mind, TickOutcome};

const AFTER_JOB_PAUSE: Duration = Duration::from_secs(3);
const AFTER_ERROR_PAUSE: Duration = Duration::from_secs(60);

pub async fn run(mind: Arc<Mind>, mut shutdown: watch::Receiver<bool>) {
    let tick = Duration::from_secs(mind.cfg.modes.tick_seconds.max(5));
    let mut next_delay = Duration::from_secs(2);
    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
            _ = tokio::time::sleep(next_delay) => {}
        }
        if *shutdown.borrow() {
            break;
        }
        next_delay = match mind.tick().await {
            Ok(TickOutcome::Ran { success: true, .. }) => AFTER_JOB_PAUSE,
            Ok(TickOutcome::Ran { success: false, .. }) => AFTER_ERROR_PAUSE,
            Ok(TickOutcome::Preempted { .. }) => tick,
            Ok(TickOutcome::Busy) => tick,
            Ok(TickOutcome::NothingToDo) => tick,
            Ok(TickOutcome::Waiting { .. }) => tick,
            Err(e) => {
                tracing::error!("scheduler tick failed: {e}");
                AFTER_ERROR_PAUSE
            }
        };
    }
    tracing::info!("scheduler stopped");
}
