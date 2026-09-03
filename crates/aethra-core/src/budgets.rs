//! Daily resource ledgers. The scheduler and tools consult these before
//! spending; limits come from config and are therefore Tier C.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::config::BudgetsConfig;
use crate::error::{CoreError, Result};
use crate::util::today_local;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Resource {
    LearningTokens,
    HttpRequests,
    HttpBytes,
    LearningMinutes,
    ResearchJobs,
}

impl Resource {
    pub const ALL: [Resource; 5] = [
        Resource::LearningTokens,
        Resource::HttpRequests,
        Resource::HttpBytes,
        Resource::LearningMinutes,
        Resource::ResearchJobs,
    ];

    pub fn key(&self) -> &'static str {
        match self {
            Resource::LearningTokens => "learning_tokens",
            Resource::HttpRequests => "http_requests",
            Resource::HttpBytes => "http_bytes",
            Resource::LearningMinutes => "learning_minutes",
            Resource::ResearchJobs => "research_jobs",
        }
    }

    pub fn limit(&self, cfg: &BudgetsConfig) -> u64 {
        match self {
            Resource::LearningTokens => cfg.learning_tokens_per_day,
            Resource::HttpRequests => cfg.http_requests_per_day,
            Resource::HttpBytes => cfg.http_bytes_per_day,
            Resource::LearningMinutes => cfg.learning_minutes_per_day,
            Resource::ResearchJobs => cfg.research_jobs_per_day,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetStatus {
    pub resource: Resource,
    pub used: u64,
    pub limit: u64,
}

pub fn used_today(conn: &Connection, res: Resource) -> Result<u64> {
    let used: Option<i64> = conn
        .query_row(
            "SELECT used FROM budgets WHERE resource = ?1 AND day = ?2",
            params![res.key(), today_local()],
            |r| r.get(0),
        )
        .ok();
    Ok(used.unwrap_or(0).max(0) as u64)
}

/// Fails without spending when `used + amount` would exceed the limit.
pub fn try_consume(conn: &Connection, cfg: &BudgetsConfig, res: Resource, amount: u64) -> Result<()> {
    let limit = res.limit(cfg);
    let used = used_today(conn, res)?;
    if used.saturating_add(amount) > limit {
        return Err(CoreError::BudgetExhausted(format!(
            "{}: {used} used + {amount} requested > {limit} per day",
            res.key()
        )));
    }
    consume_unchecked(conn, res, amount)
}

/// Records spending that already happened (for example token usage reported
/// after a completion). Never fails on the limit; the check belongs before the call.
pub fn consume_unchecked(conn: &Connection, res: Resource, amount: u64) -> Result<()> {
    conn.execute(
        "INSERT INTO budgets (resource, day, used) VALUES (?1, ?2, ?3)
         ON CONFLICT(resource, day) DO UPDATE SET used = used + excluded.used",
        params![res.key(), today_local(), amount as i64],
    )?;
    Ok(())
}

pub fn has_headroom(conn: &Connection, cfg: &BudgetsConfig, res: Resource, amount: u64) -> Result<bool> {
    let used = used_today(conn, res)?;
    Ok(used.saturating_add(amount) <= res.limit(cfg))
}

pub fn snapshot(conn: &Connection, cfg: &BudgetsConfig) -> Result<Vec<BudgetStatus>> {
    let mut out = Vec::with_capacity(Resource::ALL.len());
    for res in Resource::ALL {
        out.push(BudgetStatus {
            resource: res,
            used: used_today(conn, res)?,
            limit: res.limit(cfg),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Databases;

    #[test]
    fn ledger_enforces_limits() {
        let dbs = Databases::open_in_memory().unwrap();
        let conn = dbs.mind.lock();
        let cfg = BudgetsConfig {
            http_requests_per_day: 2,
            ..BudgetsConfig::default()
        };
        try_consume(&conn, &cfg, Resource::HttpRequests, 1).unwrap();
        try_consume(&conn, &cfg, Resource::HttpRequests, 1).unwrap();
        let err = try_consume(&conn, &cfg, Resource::HttpRequests, 1);
        assert!(matches!(err, Err(CoreError::BudgetExhausted(_))));
        assert_eq!(used_today(&conn, Resource::HttpRequests).unwrap(), 2);
    }
}
