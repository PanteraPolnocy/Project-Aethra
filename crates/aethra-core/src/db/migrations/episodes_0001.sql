-- episodes.db v1: the append-only life log. High churn, kept separate so
-- mind.db backups stay small.

CREATE TABLE episodes (
    id                TEXT PRIMARY KEY,
    kind              TEXT NOT NULL,            -- conversation | learning | system
    started_at        TEXT NOT NULL,
    ended_at          TEXT,
    summary           TEXT NOT NULL DEFAULT '',
    taint             TEXT NOT NULL DEFAULT 'self', -- user | self | web
    mode              TEXT NOT NULL,            -- chat | learning | idle
    job_id            TEXT,
    prompt_tokens     INTEGER NOT NULL DEFAULT 0,
    completion_tokens INTEGER NOT NULL DEFAULT 0,
    outcome           TEXT,
    consolidated      INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_episodes_started ON episodes(started_at DESC);
CREATE INDEX idx_episodes_pending ON episodes(consolidated, started_at);

CREATE TABLE episode_items (
    episode_id TEXT NOT NULL REFERENCES episodes(id) ON DELETE CASCADE,
    seq        INTEGER NOT NULL,
    role       TEXT NOT NULL,                   -- user | assistant | tool | system
    content    TEXT NOT NULL,
    tool_name  TEXT,
    tool_args  TEXT,
    created_at TEXT NOT NULL,
    PRIMARY KEY (episode_id, seq)
);
