-- mind.db v1: identity, curiosity, governance, scheduling.
-- Timestamps are RFC 3339 UTC strings. IDs are UUID v4 strings unless noted.

CREATE TABLE settings (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- Root goals and hard constraints authored by the user. Immutable from inside
-- the system: only set_constitution (a user action) writes here, via changes.
CREATE TABLE constitution (
    id         INTEGER PRIMARY KEY CHECK (id = 1),
    text       TEXT NOT NULL,
    version    INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- Small, slowly changing self-description. Always in context.
CREATE TABLE self_model (
    section    TEXT PRIMARY KEY,
    content    TEXT NOT NULL,
    version    INTEGER NOT NULL DEFAULT 1,
    updated_at TEXT NOT NULL
);

CREATE TABLE self_model_history (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    section     TEXT NOT NULL,
    content     TEXT NOT NULL,
    version     INTEGER NOT NULL,
    replaced_at TEXT NOT NULL,
    change_id   INTEGER
);

-- Bounded scalars in [0, 1] updated by deterministic rules, never by the model.
CREATE TABLE internal_state (
    name       TEXT PRIMARY KEY,
    value      REAL NOT NULL,
    updated_at TEXT NOT NULL,
    reason     TEXT
);

-- The curiosity queue.
CREATE TABLE questions (
    id                TEXT PRIMARY KEY,
    text              TEXT NOT NULL,
    origin            TEXT NOT NULL,             -- consolidation | research | user | reflection
    status            TEXT NOT NULL DEFAULT 'open', -- open | investigating | investigated | retired
    importance        REAL NOT NULL DEFAULT 0.5,
    tractability      REAL NOT NULL DEFAULT 0.5,
    attempts          INTEGER NOT NULL DEFAULT 0,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    source_episode_id TEXT,
    notes             TEXT
);
CREATE INDEX idx_questions_status ON questions(status, importance DESC);

-- Hierarchical autobiography: scope = day | week | month | year.
CREATE TABLE summaries (
    id            TEXT PRIMARY KEY,
    scope         TEXT NOT NULL,
    period_start  TEXT NOT NULL,
    period_end    TEXT NOT NULL,
    text          TEXT NOT NULL,
    episode_count INTEGER NOT NULL,
    created_at    TEXT NOT NULL
);
CREATE INDEX idx_summaries_period ON summaries(scope, period_end DESC);

-- Findings from reading, with the sources that were actually used.
CREATE TABLE notes (
    id           TEXT PRIMARY KEY,
    kind         TEXT NOT NULL,                  -- research | observation
    question_id  TEXT,
    title        TEXT NOT NULL,
    text         TEXT NOT NULL,
    confidence   TEXT NOT NULL,                  -- free text: what the evidence supports and what it does not
    sources_json TEXT NOT NULL DEFAULT '[]',     -- [{url, content_hash, fetched_at}]
    episode_id   TEXT,
    created_at   TEXT NOT NULL
);
CREATE INDEX idx_notes_created ON notes(created_at DESC);
CREATE INDEX idx_notes_question ON notes(question_id);

-- Append-only audit of every mutation to persistent state.
CREATE TABLE changes (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT,
    at                 TEXT NOT NULL,
    tier               TEXT NOT NULL,            -- A | B | C
    target_table       TEXT NOT NULL,
    target_id          TEXT NOT NULL,
    before_json        TEXT,
    after_json         TEXT,
    reason             TEXT NOT NULL,
    trigger_episode_id TEXT,
    approved_by        TEXT NOT NULL             -- system | user
);
CREATE INDEX idx_changes_at ON changes(at DESC);

-- Daily ledgers. day is YYYY-MM-DD in local time.
CREATE TABLE budgets (
    resource TEXT NOT NULL,
    day      TEXT NOT NULL,
    used     INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (resource, day)
);

-- Scheduler queue and history.
CREATE TABLE jobs (
    id            TEXT PRIMARY KEY,
    kind          TEXT NOT NULL,
    payload       TEXT NOT NULL,
    state         TEXT NOT NULL DEFAULT 'queued', -- queued | running | done | failed | preempted
    created_at    TEXT NOT NULL,
    started_at    TEXT,
    finished_at   TEXT,
    outcome       TEXT,
    error         TEXT,
    repeat_hash   TEXT
);
CREATE INDEX idx_jobs_state ON jobs(state, created_at);
CREATE INDEX idx_jobs_repeat ON jobs(repeat_hash, created_at DESC);
