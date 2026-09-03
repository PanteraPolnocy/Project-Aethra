//! Aethra core: the persistent mind, independent of any user interface.
//!
//! Layering, top to bottom:
//! - `mind`, `scheduler`, `jobs`: behaviour (chat turns, learning jobs, pacing)
//! - `context`, `policy`, `tools`, `model_host`: how the mind talks to the model and the world
//! - `identity`, `episodes`, `knowledge`, `state`, `budgets`, `changes`: what persists
//! - `db`, `config`, `error`, `util`: plumbing

pub mod budgets;
pub mod changes;
pub mod config;
pub mod context;
pub mod db;
pub mod episodes;
pub mod error;
pub mod events;
pub mod identity;
pub mod jobs;
pub mod knowledge;
pub mod mind;
pub mod mode;
pub mod model_host;
pub mod policy;
pub mod scheduler;
pub mod state;
pub mod tools;
pub mod util;

pub use config::AppConfig;
pub use error::{CoreError, Result};
pub use events::MindEvent;
pub use mind::{ChatReply, Mind, MindStatus, TickOutcome};
pub use mode::{LearningGate, Mode};
