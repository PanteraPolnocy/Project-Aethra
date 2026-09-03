-- cache.db v1: disposable fetched content. Deleting this file loses nothing
-- the mind depends on; evidence quotes simply become unverifiable until refetched.

CREATE TABLE page_cache (
    content_hash   TEXT PRIMARY KEY,           -- sha256 of the raw body
    url            TEXT NOT NULL,
    fetched_at     TEXT NOT NULL,
    status         INTEGER NOT NULL,
    content_type   TEXT,
    title          TEXT,
    extracted_text TEXT NOT NULL,
    byte_len       INTEGER NOT NULL
);
CREATE INDEX idx_page_cache_url ON page_cache(url, fetched_at DESC);
