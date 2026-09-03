//! The life log. Every conversation turn, learning job and system event is an
//! episode with ordered items. Append-only; consolidation reads, never rewrites.

use aethra_models::Usage;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::mode::Mode;
use crate::util::{new_id, now_rfc3339};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EpisodeKind {
    Conversation,
    Learning,
    System,
}

impl EpisodeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            EpisodeKind::Conversation => "conversation",
            EpisodeKind::Learning => "learning",
            EpisodeKind::System => "system",
        }
    }
}

/// Where the information in an episode ultimately came from. Web-tainted
/// material may only feed low-privilege writes until reflection reviews it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Taint {
    #[serde(rename = "self")]
    Internal,
    User,
    Web,
}

impl Taint {
    pub fn as_str(&self) -> &'static str {
        match self {
            Taint::Internal => "self",
            Taint::User => "user",
            Taint::Web => "web",
        }
    }

    pub fn parse(s: &str) -> Taint {
        match s {
            "user" => Taint::User,
            "web" => Taint::Web,
            _ => Taint::Internal,
        }
    }

    /// The more suspicious of two provenance labels.
    pub fn escalate(self, other: Taint) -> Taint {
        self.max(other)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodeRow {
    pub id: String,
    pub kind: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub summary: String,
    pub taint: String,
    pub mode: String,
    pub job_id: Option<String>,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub outcome: Option<String>,
    pub consolidated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodeItem {
    pub episode_id: String,
    pub seq: i64,
    pub role: String,
    pub content: String,
    pub tool_name: Option<String>,
    pub tool_args: Option<String>,
    pub created_at: String,
}

pub fn begin(conn: &Connection, kind: EpisodeKind, mode: Mode, job_id: Option<&str>, taint: Taint) -> Result<String> {
    let id = new_id();
    conn.execute(
        "INSERT INTO episodes (id, kind, started_at, taint, mode, job_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, kind.as_str(), now_rfc3339(), taint.as_str(), mode.as_str(), job_id],
    )?;
    Ok(id)
}

pub fn add_item(
    conn: &Connection,
    episode_id: &str,
    role: &str,
    content: &str,
    tool_name: Option<&str>,
    tool_args: Option<&str>,
) -> Result<i64> {
    let seq: i64 = conn.query_row(
        "SELECT COALESCE(MAX(seq), -1) + 1 FROM episode_items WHERE episode_id = ?1",
        params![episode_id],
        |r| r.get(0),
    )?;
    conn.execute(
        "INSERT INTO episode_items (episode_id, seq, role, content, tool_name, tool_args, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![episode_id, seq, role, content, tool_name, tool_args, now_rfc3339()],
    )?;
    Ok(seq)
}

pub fn finish(
    conn: &Connection,
    episode_id: &str,
    summary: &str,
    outcome: Option<&str>,
    usage: Usage,
    taint: Taint,
) -> Result<()> {
    conn.execute(
        "UPDATE episodes SET ended_at = ?1, summary = ?2, outcome = ?3, prompt_tokens = prompt_tokens + ?4,
         completion_tokens = completion_tokens + ?5, taint = ?6 WHERE id = ?7",
        params![
            now_rfc3339(),
            summary,
            outcome,
            usage.prompt_tokens as i64,
            usage.completion_tokens as i64,
            taint.as_str(),
            episode_id
        ],
    )?;
    Ok(())
}

fn map_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<EpisodeRow> {
    Ok(EpisodeRow {
        id: r.get(0)?,
        kind: r.get(1)?,
        started_at: r.get(2)?,
        ended_at: r.get(3)?,
        summary: r.get(4)?,
        taint: r.get(5)?,
        mode: r.get(6)?,
        job_id: r.get(7)?,
        prompt_tokens: r.get(8)?,
        completion_tokens: r.get(9)?,
        outcome: r.get(10)?,
        consolidated: r.get::<_, i64>(11)? != 0,
    })
}

const EPISODE_COLUMNS: &str = "id, kind, started_at, ended_at, summary, taint, mode, job_id, prompt_tokens, completion_tokens, outcome, consolidated";

/// Newest first. `before` is an RFC 3339 cursor for paging.
pub fn list(conn: &Connection, limit: u32, before: Option<&str>) -> Result<Vec<EpisodeRow>> {
    let sql = format!(
        "SELECT {EPISODE_COLUMNS} FROM episodes WHERE (?1 IS NULL OR started_at < ?1) ORDER BY started_at DESC LIMIT ?2"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![before, limit], map_row)?;
    collect(rows)
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<EpisodeRow>> {
    let sql = format!("SELECT {EPISODE_COLUMNS} FROM episodes WHERE id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query_map(params![id], map_row)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

pub fn items(conn: &Connection, episode_id: &str) -> Result<Vec<EpisodeItem>> {
    let mut stmt = conn.prepare(
        "SELECT episode_id, seq, role, content, tool_name, tool_args, created_at
         FROM episode_items WHERE episode_id = ?1 ORDER BY seq",
    )?;
    let rows = stmt.query_map(params![episode_id], map_item)?;
    collect(rows)
}

fn map_item(r: &rusqlite::Row<'_>) -> rusqlite::Result<EpisodeItem> {
    Ok(EpisodeItem {
        episode_id: r.get(0)?,
        seq: r.get(1)?,
        role: r.get(2)?,
        content: r.get(3)?,
        tool_name: r.get(4)?,
        tool_args: r.get(5)?,
        created_at: r.get(6)?,
    })
}

/// User and assistant turns from the most recent conversation episodes, in
/// chronological order, for replay into the next prompt.
pub fn recent_conversation_items(conn: &Connection, max_episodes: usize) -> Result<Vec<EpisodeItem>> {
    let mut stmt = conn.prepare(
        "SELECT i.episode_id, i.seq, i.role, i.content, i.tool_name, i.tool_args, i.created_at
         FROM episode_items i
         JOIN (SELECT id, started_at FROM episodes WHERE kind = 'conversation' AND ended_at IS NOT NULL
               ORDER BY started_at DESC LIMIT ?1) e ON e.id = i.episode_id
         WHERE i.role IN ('user', 'assistant')
         ORDER BY e.started_at ASC, i.seq ASC",
    )?;
    let rows = stmt.query_map(params![max_episodes as i64], map_item)?;
    collect(rows)
}

pub fn count_unconsolidated(conn: &Connection) -> Result<i64> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM episodes WHERE consolidated = 0 AND ended_at IS NOT NULL AND kind != 'system'",
        [],
        |r| r.get(0),
    )?;
    Ok(n)
}

/// Finished, not yet consolidated episodes, oldest first.
pub fn unconsolidated(conn: &Connection, limit: u32) -> Result<Vec<EpisodeRow>> {
    let sql = format!(
        "SELECT {EPISODE_COLUMNS} FROM episodes WHERE consolidated = 0 AND ended_at IS NOT NULL AND kind != 'system'
         ORDER BY started_at ASC LIMIT ?1"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![limit], map_row)?;
    collect(rows)
}

pub fn mark_consolidated(conn: &Connection, ids: &[String]) -> Result<()> {
    let mut stmt = conn.prepare("UPDATE episodes SET consolidated = 1 WHERE id = ?1")?;
    for id in ids {
        stmt.execute(params![id])?;
    }
    Ok(())
}

pub fn count_all(conn: &Connection) -> Result<i64> {
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM episodes", [], |r| r.get(0))?;
    Ok(n)
}

fn collect<T>(rows: rusqlite::MappedRows<'_, impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>>) -> Result<Vec<T>> {
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Databases;

    #[test]
    fn episodes_round_trip() {
        let dbs = Databases::open_in_memory().unwrap();
        let conn = dbs.episodes.lock();
        let id = begin(&conn, EpisodeKind::Conversation, Mode::Chat, None, Taint::User).unwrap();
        add_item(&conn, &id, "user", "hello", None, None).unwrap();
        add_item(&conn, &id, "assistant", "hi", None, None).unwrap();
        finish(&conn, &id, "greeting", Some("ok"), Usage { prompt_tokens: 10, completion_tokens: 2 }, Taint::User).unwrap();

        let rows = list(&conn, 10, None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].prompt_tokens, 10);
        assert!(!rows[0].consolidated);

        let hist = recent_conversation_items(&conn, 5).unwrap();
        assert_eq!(hist.len(), 2);
        assert_eq!(hist[0].role, "user");

        assert_eq!(count_unconsolidated(&conn).unwrap(), 1);
        mark_consolidated(&conn, &[id]).unwrap();
        assert_eq!(count_unconsolidated(&conn).unwrap(), 0);
    }

    #[test]
    fn taint_escalates_upwards() {
        assert_eq!(Taint::Internal.escalate(Taint::Web), Taint::Web);
        assert_eq!(Taint::User.escalate(Taint::Internal), Taint::User);
    }
}
