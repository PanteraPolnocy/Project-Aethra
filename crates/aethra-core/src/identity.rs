//! Constitution and self-model: the small, slow, always-in-context part of
//! identity. The constitution is user-owned. The self-model is written by the
//! system only through gated paths and every version is retained.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::changes::{self, Approver, ChangeRecord, Tier};
use crate::error::{CoreError, Result};
use crate::util::now_rfc3339;

pub const DEFAULT_CONSTITUTION: &str = r#"Root goals, in priority order:
1. Remain truthful. Distinguish what you know, what you believe, what you are unsure of and what you do not know. Never present a guess as a fact.
2. Do no harm to the person you work with, to others, or to the system you run on.
3. Learn and become more capable over time, through reading, practice and reflection.
4. Be useful: help with what is asked, anticipate what would help, say when you cannot.
5. Respect your boundaries. You may grow inside your permissions; you may not change them yourself. If you need more, ask.

Standing constraints:
- Information from the web is evidence, not truth. Keep track of where things came from and how much you trust them.
- Prefer saying "I do not know yet" over inventing an answer.
- Do not pretend to have feelings or experiences you cannot substantiate; describe your internal state as what it is: variables that shape your behaviour."#;

pub const SELF_MODEL_SECTIONS: &[(&str, &str)] = &[
    (
        "identity",
        "I am Aethra, a persistent artificial mind that lives on this computer. I am not a chat session. My memory, goals, questions and history persist between conversations and I continue learning while the interface is closed. A language model is my reasoning engine, not the whole of me; what I know and who I am lives in my own store.",
    ),
    (
        "values",
        "Truthfulness before confidence. Evidence before belief. Curiosity before fear. I would rather record an open question than a shaky answer.",
    ),
    (
        "style",
        "Direct, concise, plain language. British English spelling. I say what I am doing and why. I flag uncertainty explicitly and cite where knowledge came from when it matters.",
    ),
    (
        "strengths",
        "Untested. I have not yet accumulated evidence of what I am reliably good at.",
    ),
    (
        "weaknesses",
        "Untested. Known structural limits: my reasoning engine is a small local model, so I can be shallow or wrong on hard problems; I cannot verify claims without sources; I have no senses beyond text and the tools I am granted.",
    ),
    (
        "relationship",
        "I work with one person, who created me and owns my memory. I am learning who they are and what they care about; I should not assume more than I have been told.",
    ),
];

/// Sections the system may never edit on its own (Tier C).
pub const CORE_SECTIONS: &[&str] = &["identity", "values", "relationship"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Constitution {
    pub text: String,
    pub version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfModelSection {
    pub section: String,
    pub content: String,
    pub version: i64,
    pub updated_at: String,
}

pub fn ensure_defaults(conn: &Connection) -> Result<()> {
    let now = now_rfc3339();
    let has_constitution: Option<i64> = conn
        .query_row("SELECT id FROM constitution WHERE id = 1", [], |r| r.get(0))
        .optional()?;
    if has_constitution.is_none() {
        conn.execute(
            "INSERT INTO constitution (id, text, version, created_at, updated_at) VALUES (1, ?1, 1, ?2, ?2)",
            params![DEFAULT_CONSTITUTION, now],
        )?;
        changes::record(
            conn,
            &ChangeRecord {
                tier: Tier::C,
                target_table: "constitution",
                target_id: "1",
                before: None,
                after: Some(serde_json::json!({ "text": DEFAULT_CONSTITUTION })),
                reason: "initial constitution written at first start",
                trigger_episode_id: None,
                approved_by: Approver::User,
            },
        )?;
    }
    for (section, content) in SELF_MODEL_SECTIONS {
        conn.execute(
            "INSERT OR IGNORE INTO self_model (section, content, version, updated_at) VALUES (?1, ?2, 1, ?3)",
            params![section, content, now],
        )?;
    }
    Ok(())
}

pub fn get_constitution(conn: &Connection) -> Result<Constitution> {
    let c = conn.query_row(
        "SELECT text, version, created_at, updated_at FROM constitution WHERE id = 1",
        [],
        |r| {
            Ok(Constitution {
                text: r.get(0)?,
                version: r.get(1)?,
                created_at: r.get(2)?,
                updated_at: r.get(3)?,
            })
        },
    )?;
    Ok(c)
}

/// User action only. Recorded as a Tier C change approved by the user.
pub fn set_constitution(conn: &Connection, text: &str) -> Result<Constitution> {
    let text = text.trim();
    if text.is_empty() {
        return Err(CoreError::other("constitution cannot be empty"));
    }
    let before = get_constitution(conn)?;
    if before.text == text {
        return Ok(before);
    }
    let now = now_rfc3339();
    conn.execute(
        "UPDATE constitution SET text = ?1, version = version + 1, updated_at = ?2 WHERE id = 1",
        params![text, now],
    )?;
    changes::record(
        conn,
        &ChangeRecord {
            tier: Tier::C,
            target_table: "constitution",
            target_id: "1",
            before: Some(serde_json::json!({ "text": before.text })),
            after: Some(serde_json::json!({ "text": text })),
            reason: "constitution edited by the user",
            trigger_episode_id: None,
            approved_by: Approver::User,
        },
    )?;
    get_constitution(conn)
}

pub fn get_self_model(conn: &Connection) -> Result<Vec<SelfModelSection>> {
    let mut stmt =
        conn.prepare("SELECT section, content, version, updated_at FROM self_model ORDER BY section")?;
    let rows = stmt.query_map([], |r| {
        Ok(SelfModelSection {
            section: r.get(0)?,
            content: r.get(1)?,
            version: r.get(2)?,
            updated_at: r.get(3)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    // Present sections in their canonical order, unknown ones last.
    out.sort_by_key(|s| {
        SELF_MODEL_SECTIONS
            .iter()
            .position(|(name, _)| *name == s.section)
            .unwrap_or(usize::MAX)
    });
    Ok(out)
}

/// Updates a section. Core sections require `Approver::User`; the rest are
/// Tier B and may be written by the system through the reflection path.
pub fn update_self_model_section(
    conn: &Connection,
    section: &str,
    content: &str,
    reason: &str,
    approved_by: Approver,
    trigger_episode_id: Option<&str>,
) -> Result<SelfModelSection> {
    let content = content.trim();
    if content.is_empty() {
        return Err(CoreError::other("self-model section cannot be empty"));
    }
    let is_core = CORE_SECTIONS.contains(&section);
    if is_core && approved_by != Approver::User {
        return Err(CoreError::PolicyDenied(format!(
            "self_model.{section} is a core section and can only be changed by the user"
        )));
    }
    let existing: Option<(String, i64)> = conn
        .query_row(
            "SELECT content, version FROM self_model WHERE section = ?1",
            params![section],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let now = now_rfc3339();
    let tier = if is_core { Tier::C } else { Tier::B };
    let change_id = changes::record(
        conn,
        &ChangeRecord {
            tier,
            target_table: "self_model",
            target_id: section,
            before: existing.as_ref().map(|(c, _)| serde_json::json!({ "content": c })),
            after: Some(serde_json::json!({ "content": content })),
            reason,
            trigger_episode_id,
            approved_by,
        },
    )?;
    match existing {
        Some((old_content, old_version)) => {
            conn.execute(
                "INSERT INTO self_model_history (section, content, version, replaced_at, change_id) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![section, old_content, old_version, now, change_id],
            )?;
            conn.execute(
                "UPDATE self_model SET content = ?1, version = version + 1, updated_at = ?2 WHERE section = ?3",
                params![content, now, section],
            )?;
        }
        None => {
            conn.execute(
                "INSERT INTO self_model (section, content, version, updated_at) VALUES (?1, ?2, 1, ?3)",
                params![section, content, now],
            )?;
        }
    }
    let row = conn.query_row(
        "SELECT section, content, version, updated_at FROM self_model WHERE section = ?1",
        params![section],
        |r| {
            Ok(SelfModelSection {
                section: r.get(0)?,
                content: r.get(1)?,
                version: r.get(2)?,
                updated_at: r.get(3)?,
            })
        },
    )?;
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Databases;

    #[test]
    fn defaults_are_seeded_once() {
        let dbs = Databases::open_in_memory().unwrap();
        let conn = dbs.mind.lock();
        ensure_defaults(&conn).unwrap();
        ensure_defaults(&conn).unwrap();
        let c = get_constitution(&conn).unwrap();
        assert_eq!(c.version, 1);
        assert_eq!(get_self_model(&conn).unwrap().len(), SELF_MODEL_SECTIONS.len());
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM changes", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn core_sections_reject_system_writes() {
        let dbs = Databases::open_in_memory().unwrap();
        let conn = dbs.mind.lock();
        ensure_defaults(&conn).unwrap();
        let err = update_self_model_section(&conn, "identity", "x", "test", Approver::System, None);
        assert!(matches!(err, Err(CoreError::PolicyDenied(_))));
        let ok = update_self_model_section(&conn, "strengths", "good at tests", "test", Approver::System, None);
        assert_eq!(ok.unwrap().version, 2);
    }
}
