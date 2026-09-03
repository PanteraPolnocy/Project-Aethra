//! SQLite storage. Three files, one writer (this process), WAL everywhere.
//!
//! - mind.db     identity, knowledge, agency, governance (back this up)
//! - episodes.db the life log (large, high churn)
//! - cache.db    fetched pages (disposable)

use std::path::{Path, PathBuf};
use std::sync::Once;
use std::time::Duration;

use parking_lot::Mutex;
use rusqlite::Connection;

use crate::config::AppConfig;
use crate::error::Result;
use crate::util::now_rfc3339;

const MIND_MIGRATIONS: &[&str] = &[include_str!("migrations/mind_0001.sql")];
const EPISODES_MIGRATIONS: &[&str] = &[include_str!("migrations/episodes_0001.sql")];
const CACHE_MIGRATIONS: &[&str] = &[include_str!("migrations/cache_0001.sql")];

pub struct Databases {
    pub mind: Mutex<Connection>,
    pub episodes: Mutex<Connection>,
    pub cache: Mutex<Connection>,
}

static REGISTER: Once = Once::new();

/// The signature SQLite actually calls an extension entry point with.
type ExtensionInit = unsafe extern "C" fn(
    *mut rusqlite::ffi::sqlite3,
    *mut *mut std::os::raw::c_char,
    *const rusqlite::ffi::sqlite3_api_routines,
) -> std::os::raw::c_int;

/// Registers sqlite-vec as an auto-extension so every connection opened
/// afterwards has `vec0` and `vec_*` available. Safe to call repeatedly.
pub fn register_extensions() {
    REGISTER.call_once(|| unsafe {
        // sqlite-vec exports the entry point with an erased `fn()` signature;
        // the C symbol has the standard one, so the cast is sound.
        let init = std::mem::transmute::<unsafe extern "C" fn(), ExtensionInit>(sqlite_vec::sqlite3_vec_init);
        rusqlite::ffi::sqlite3_auto_extension(Some(init));
    });
}

impl Databases {
    pub fn open(cfg: &AppConfig) -> Result<Self> {
        register_extensions();
        std::fs::create_dir_all(&cfg.data_dir)?;
        let mind = open_one(&cfg.mind_db_path(), MIND_MIGRATIONS)?;
        let episodes = open_one(&cfg.episodes_db_path(), EPISODES_MIGRATIONS)?;
        let cache = open_one(&cfg.cache_db_path(), CACHE_MIGRATIONS)?;

        let vec_version: String = mind.query_row("SELECT vec_version()", [], |r| r.get(0))?;
        tracing::info!(sqlite = rusqlite::version(), sqlite_vec = %vec_version, "databases open");

        Ok(Self {
            mind: Mutex::new(mind),
            episodes: Mutex::new(episodes),
            cache: Mutex::new(cache),
        })
    }

    /// In-memory databases for tests.
    pub fn open_in_memory() -> Result<Self> {
        register_extensions();
        Ok(Self {
            mind: Mutex::new(open_memory(MIND_MIGRATIONS)?),
            episodes: Mutex::new(open_memory(EPISODES_MIGRATIONS)?),
            cache: Mutex::new(open_memory(CACHE_MIGRATIONS)?),
        })
    }

    /// Consistent point-in-time copies via `VACUUM INTO`. Returns written paths.
    pub fn snapshot(&self, dir: &Path) -> Result<Vec<PathBuf>> {
        std::fs::create_dir_all(dir)?;
        let stamp = now_rfc3339().replace([':', '.'], "-");
        let mut out = Vec::new();
        for (name, conn) in [("mind", &self.mind), ("episodes", &self.episodes)] {
            let target = dir.join(format!("{name}-{stamp}.db"));
            let escaped = target.to_string_lossy().replace('\'', "''");
            conn.lock().execute_batch(&format!("VACUUM INTO '{escaped}'"))?;
            out.push(target);
        }
        Ok(out)
    }
}

fn open_one(path: &Path, migrations: &[&str]) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.busy_timeout(Duration::from_secs(5))?;
    migrate(&conn, migrations)?;
    Ok(conn)
}

fn open_memory(migrations: &[&str]) -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrate(&conn, migrations)?;
    Ok(conn)
}

fn migrate(conn: &Connection, migrations: &[&str]) -> Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (idx, sql) in migrations.iter().enumerate() {
        let target = idx as i64 + 1;
        if target <= current {
            continue;
        }
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", target)?;
        tx.commit()?;
        tracing::info!(version = target, "applied migration");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_apply_and_vec_loads() {
        let dbs = Databases::open_in_memory().unwrap();
        let v: i64 = dbs
            .mind
            .lock()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, 1);
        let vec: String = dbs
            .mind
            .lock()
            .query_row("SELECT vec_version()", [], |r| r.get(0))
            .unwrap();
        assert!(vec.starts_with('v'));
    }
}
