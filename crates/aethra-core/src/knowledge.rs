//! Questions (the curiosity queue), summaries (autobiography) and notes
//! (findings with sources). All Tier A writes, all audited.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::changes::{self, Approver, ChangeRecord, Tier};
use crate::error::Result;
use crate::util::{clamp01, new_id, now_rfc3339};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Question {
    pub id: String,
    pub text: String,
    pub origin: String,
    pub status: String,
    pub importance: f64,
    pub tractability: f64,
    pub attempts: i64,
    pub created_at: String,
    pub updated_at: String,
    pub source_episode_id: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    pub id: String,
    pub scope: String,
    pub period_start: String,
    pub period_end: String,
    pub text: String,
    pub episode_count: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteSource {
    pub url: String,
    pub content_hash: String,
    pub fetched_at: String,
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub id: String,
    pub kind: String,
    pub question_id: Option<String>,
    pub title: String,
    pub text: String,
    pub confidence: String,
    pub sources: Vec<NoteSource>,
    pub episode_id: Option<String>,
    pub created_at: String,
}

fn normalise(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_end_matches(['?', '.', '!'])
        .to_lowercase()
}

/// Adds a question unless an equivalent open one exists. Returns `None` when deduplicated.
pub fn add_question(
    conn: &Connection,
    text: &str,
    origin: &str,
    importance: f64,
    source_episode_id: Option<&str>,
) -> Result<Option<Question>> {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.len() < 8 {
        return Ok(None);
    }
    let norm = normalise(&text);
    let mut stmt = conn.prepare("SELECT text FROM questions WHERE status IN ('open', 'investigating')")?;
    let existing = stmt.query_map([], |r| r.get::<_, String>(0))?;
    for row in existing {
        if normalise(&row?) == norm {
            return Ok(None);
        }
    }
    let id = new_id();
    let now = now_rfc3339();
    let importance = clamp01(importance);
    conn.execute(
        "INSERT INTO questions (id, text, origin, status, importance, tractability, attempts, created_at, updated_at, source_episode_id)
         VALUES (?1, ?2, ?3, 'open', ?4, 0.5, 0, ?5, ?5, ?6)",
        params![id, text, origin, importance, now, source_episode_id],
    )?;
    changes::record(
        conn,
        &ChangeRecord {
            tier: Tier::A,
            target_table: "questions",
            target_id: &id,
            before: None,
            after: Some(serde_json::json!({ "text": text, "origin": origin, "importance": importance })),
            reason: "question generated",
            trigger_episode_id: source_episode_id,
            approved_by: Approver::System,
        },
    )?;
    get_question(conn, &id)
}

fn map_question(r: &rusqlite::Row<'_>) -> rusqlite::Result<Question> {
    Ok(Question {
        id: r.get(0)?,
        text: r.get(1)?,
        origin: r.get(2)?,
        status: r.get(3)?,
        importance: r.get(4)?,
        tractability: r.get(5)?,
        attempts: r.get(6)?,
        created_at: r.get(7)?,
        updated_at: r.get(8)?,
        source_episode_id: r.get(9)?,
        notes: r.get(10)?,
    })
}

const QUESTION_COLUMNS: &str =
    "id, text, origin, status, importance, tractability, attempts, created_at, updated_at, source_episode_id, notes";

pub fn get_question(conn: &Connection, id: &str) -> Result<Option<Question>> {
    let sql = format!("SELECT {QUESTION_COLUMNS} FROM questions WHERE id = ?1");
    Ok(conn.query_row(&sql, params![id], map_question).optional()?)
}

pub fn list_questions(conn: &Connection, status: Option<&str>, limit: u32) -> Result<Vec<Question>> {
    let sql = format!(
        "SELECT {QUESTION_COLUMNS} FROM questions WHERE (?1 IS NULL OR status = ?1)
         ORDER BY CASE status WHEN 'open' THEN 0 WHEN 'investigating' THEN 1 ELSE 2 END, importance DESC, created_at DESC
         LIMIT ?2"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![status, limit], map_question)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn count_open(conn: &Connection) -> Result<i64> {
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM questions WHERE status = 'open'", [], |r| r.get(0))?;
    Ok(n)
}

/// Highest expected value first: importance x tractability, fewer attempts preferred.
pub fn next_open_question(conn: &Connection, max_attempts: i64) -> Result<Option<Question>> {
    let sql = format!(
        "SELECT {QUESTION_COLUMNS} FROM questions WHERE status = 'open' AND attempts < ?1
         ORDER BY (importance * tractability) DESC, attempts ASC, created_at ASC LIMIT 1"
    );
    Ok(conn.query_row(&sql, params![max_attempts], map_question).optional()?)
}

pub fn update_question(
    conn: &Connection,
    id: &str,
    status: &str,
    notes: Option<&str>,
    tractability: Option<f64>,
    increment_attempts: bool,
    reason: &str,
    trigger_episode_id: Option<&str>,
) -> Result<()> {
    let before = get_question(conn, id)?;
    conn.execute(
        "UPDATE questions SET status = ?1, notes = COALESCE(?2, notes), tractability = COALESCE(?3, tractability),
         attempts = attempts + ?4, updated_at = ?5 WHERE id = ?6",
        params![
            status,
            notes,
            tractability.map(clamp01),
            if increment_attempts { 1 } else { 0 },
            now_rfc3339(),
            id
        ],
    )?;
    changes::record(
        conn,
        &ChangeRecord {
            tier: Tier::A,
            target_table: "questions",
            target_id: id,
            before: before.map(|q| serde_json::json!({ "status": q.status, "attempts": q.attempts })),
            after: Some(serde_json::json!({ "status": status })),
            reason,
            trigger_episode_id,
            approved_by: Approver::System,
        },
    )?;
    Ok(())
}

pub fn add_summary(
    conn: &Connection,
    scope: &str,
    period_start: &str,
    period_end: &str,
    text: &str,
    episode_count: i64,
) -> Result<Summary> {
    let id = new_id();
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO summaries (id, scope, period_start, period_end, text, episode_count, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![id, scope, period_start, period_end, text, episode_count, now],
    )?;
    changes::record(
        conn,
        &ChangeRecord {
            tier: Tier::A,
            target_table: "summaries",
            target_id: &id,
            before: None,
            after: Some(serde_json::json!({ "scope": scope, "period_end": period_end })),
            reason: "summary written by consolidation",
            trigger_episode_id: None,
            approved_by: Approver::System,
        },
    )?;
    Ok(Summary {
        id,
        scope: scope.to_string(),
        period_start: period_start.to_string(),
        period_end: period_end.to_string(),
        text: text.to_string(),
        episode_count,
        created_at: now,
    })
}

pub fn list_summaries(conn: &Connection, scope: Option<&str>, limit: u32) -> Result<Vec<Summary>> {
    let mut stmt = conn.prepare(
        "SELECT id, scope, period_start, period_end, text, episode_count, created_at FROM summaries
         WHERE (?1 IS NULL OR scope = ?1) ORDER BY period_end DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![scope, limit], |r| {
        Ok(Summary {
            id: r.get(0)?,
            scope: r.get(1)?,
            period_start: r.get(2)?,
            period_end: r.get(3)?,
            text: r.get(4)?,
            episode_count: r.get(5)?,
            created_at: r.get(6)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn add_note(
    conn: &Connection,
    kind: &str,
    question_id: Option<&str>,
    title: &str,
    text: &str,
    confidence: &str,
    sources: &[NoteSource],
    episode_id: Option<&str>,
) -> Result<Note> {
    let id = new_id();
    let now = now_rfc3339();
    let sources_json = serde_json::to_string(sources)?;
    conn.execute(
        "INSERT INTO notes (id, kind, question_id, title, text, confidence, sources_json, episode_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![id, kind, question_id, title, text, confidence, sources_json, episode_id, now],
    )?;
    changes::record(
        conn,
        &ChangeRecord {
            tier: Tier::A,
            target_table: "notes",
            target_id: &id,
            before: None,
            after: Some(serde_json::json!({ "title": title, "kind": kind, "sources": sources.len() })),
            reason: "note written",
            trigger_episode_id: episode_id,
            approved_by: Approver::System,
        },
    )?;
    Ok(Note {
        id,
        kind: kind.to_string(),
        question_id: question_id.map(str::to_string),
        title: title.to_string(),
        text: text.to_string(),
        confidence: confidence.to_string(),
        sources: sources.to_vec(),
        episode_id: episode_id.map(str::to_string),
        created_at: now,
    })
}

fn map_note(r: &rusqlite::Row<'_>) -> rusqlite::Result<Note> {
    let sources_json: String = r.get(6)?;
    Ok(Note {
        id: r.get(0)?,
        kind: r.get(1)?,
        question_id: r.get(2)?,
        title: r.get(3)?,
        text: r.get(4)?,
        confidence: r.get(5)?,
        sources: serde_json::from_str(&sources_json).unwrap_or_default(),
        episode_id: r.get(7)?,
        created_at: r.get(8)?,
    })
}

pub fn list_notes(conn: &Connection, limit: u32) -> Result<Vec<Note>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, question_id, title, text, confidence, sources_json, episode_id, created_at
         FROM notes ORDER BY created_at DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit], map_note)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn count_notes(conn: &Connection) -> Result<i64> {
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM notes", [], |r| r.get(0))?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Databases;

    #[test]
    fn questions_deduplicate_and_prioritise() {
        let dbs = Databases::open_in_memory().unwrap();
        let conn = dbs.mind.lock();
        let a = add_question(&conn, "How does SQLite WAL mode work?", "user", 0.9, None).unwrap();
        assert!(a.is_some());
        let dup = add_question(&conn, "how does sqlite WAL mode  work", "user", 0.2, None).unwrap();
        assert!(dup.is_none());
        let b = add_question(&conn, "What is information theory?", "consolidation", 0.3, None).unwrap();
        assert!(b.is_some());
        let next = next_open_question(&conn, 3).unwrap().unwrap();
        assert_eq!(next.id, a.unwrap().id);
        assert_eq!(count_open(&conn).unwrap(), 2);
    }
}
