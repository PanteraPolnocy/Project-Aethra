//! Append-only audit log. Every mutation of persistent state that is not a
//! plain episode record goes through here, with its tier and its approver.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::util::now_rfc3339;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tier {
    /// Autonomous and logged: memory content, questions, task goals, state.
    A,
    /// Two-phase self-change with cooldown: heuristics, project goals, peripheral self-model.
    B,
    /// Proposal only; the user applies: constitution, core self-model, permissions, budgets.
    C,
}

impl Tier {
    pub fn as_str(&self) -> &'static str {
        match self {
            Tier::A => "A",
            Tier::B => "B",
            Tier::C => "C",
        }
    }

    pub fn parse(s: &str) -> Option<Tier> {
        match s {
            "A" => Some(Tier::A),
            "B" => Some(Tier::B),
            "C" => Some(Tier::C),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Approver {
    System,
    User,
}

impl Approver {
    pub fn as_str(&self) -> &'static str {
        match self {
            Approver::System => "system",
            Approver::User => "user",
        }
    }
}

pub struct ChangeRecord<'a> {
    pub tier: Tier,
    pub target_table: &'a str,
    pub target_id: &'a str,
    pub before: Option<serde_json::Value>,
    pub after: Option<serde_json::Value>,
    pub reason: &'a str,
    pub trigger_episode_id: Option<&'a str>,
    pub approved_by: Approver,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeRow {
    pub id: i64,
    pub at: String,
    pub tier: String,
    pub target_table: String,
    pub target_id: String,
    pub before_json: Option<String>,
    pub after_json: Option<String>,
    pub reason: String,
    pub trigger_episode_id: Option<String>,
    pub approved_by: String,
}

pub fn record(conn: &Connection, c: &ChangeRecord<'_>) -> Result<i64> {
    conn.execute(
        "INSERT INTO changes (at, tier, target_table, target_id, before_json, after_json, reason, trigger_episode_id, approved_by)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            now_rfc3339(),
            c.tier.as_str(),
            c.target_table,
            c.target_id,
            c.before.as_ref().map(|v| v.to_string()),
            c.after.as_ref().map(|v| v.to_string()),
            c.reason,
            c.trigger_episode_id,
            c.approved_by.as_str(),
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_recent(conn: &Connection, limit: u32) -> Result<Vec<ChangeRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, at, tier, target_table, target_id, before_json, after_json, reason, trigger_episode_id, approved_by
         FROM changes ORDER BY id DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit], |r| {
        Ok(ChangeRow {
            id: r.get(0)?,
            at: r.get(1)?,
            tier: r.get(2)?,
            target_table: r.get(3)?,
            target_id: r.get(4)?,
            before_json: r.get(5)?,
            after_json: r.get(6)?,
            reason: r.get(7)?,
            trigger_episode_id: r.get(8)?,
            approved_by: r.get(9)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// Number of Tier-A changes recorded today (local day), used as a runaway brake.
pub fn count_today(conn: &Connection, tier: Tier) -> Result<i64> {
    let day_prefix = crate::util::today_local();
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM changes WHERE tier = ?1 AND substr(at, 1, 10) = ?2",
        params![tier.as_str(), day_prefix],
        |r| r.get(0),
    )?;
    Ok(n)
}
