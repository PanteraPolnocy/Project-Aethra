use aethra_models::ModelError;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("configuration error: {0}")]
    Config(String),
    #[error("model error: {0}")]
    Model(#[from] ModelError),
    #[error("policy denied: {0}")]
    PolicyDenied(String),
    #[error("budget exhausted: {0}")]
    BudgetExhausted(String),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("interrupted: {0}")]
    Interrupted(String),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, CoreError>;

impl CoreError {
    pub fn other(msg: impl Into<String>) -> Self {
        CoreError::Other(msg.into())
    }
}
