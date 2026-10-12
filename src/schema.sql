
CREATE TABLE IF NOT EXISTS sources (
    name TEXT PRIMARY KEY
);

CREATE TABLE IF NOT EXISTS sessions (
    source          TEXT NOT NULL,
    id              TEXT NOT NULL,
    started_at      TEXT,
    ended_at        TEXT,
    cwd             TEXT,
    model           TEXT,
    model_provider  TEXT,
    cli_version     TEXT,
    runtime_source  TEXT,
    originator      TEXT,
    git_branch      TEXT,
    entrypoint      TEXT,
    user_type       TEXT,
    raw_path        TEXT NOT NULL,
    file_mtime      REAL NOT NULL,
    message_count           INTEGER NOT NULL DEFAULT 0,
    user_message_count      INTEGER NOT NULL DEFAULT 0,
    assistant_message_count INTEGER NOT NULL DEFAULT 0,
    input_tokens            INTEGER NOT NULL DEFAULT 0,
    output_tokens           INTEGER NOT NULL DEFAULT 0,
    reasoning_tokens        INTEGER NOT NULL DEFAULT 0,
    cached_input_tokens     INTEGER NOT NULL DEFAULT 0,
    preview      TEXT,
    extra_json   TEXT,
    ingested_at  TEXT NOT NULL,
    PRIMARY KEY (source, id),
    FOREIGN KEY (source) REFERENCES sources(name) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_sessions_source  ON sessions(source);
CREATE INDEX IF NOT EXISTS idx_sessions_started ON sessions(started_at);
CREATE INDEX IF NOT EXISTS idx_sessions_cwd     ON sessions(cwd);

CREATE TABLE IF NOT EXISTS messages (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id      TEXT NOT NULL,
    source          TEXT NOT NULL,
    ordinal         INTEGER NOT NULL,
    timestamp       TEXT,
    role            TEXT,
    kind            TEXT,
    text            TEXT,
    content_json    TEXT NOT NULL,
    raw_json        TEXT NOT NULL,
    model           TEXT,
    stop_reason     TEXT,
    input_tokens           INTEGER,
    output_tokens          INTEGER,
    reasoning_tokens       INTEGER,
    cached_input_tokens    INTEGER,
    UNIQUE (session_id, source, ordinal),
    FOREIGN KEY (session_id, source) REFERENCES sessions(id, source) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id, source, ordinal);
CREATE INDEX IF NOT EXISTS idx_messages_role    ON messages(role);
CREATE INDEX IF NOT EXISTS idx_messages_kind    ON messages(kind);

CREATE TABLE IF NOT EXISTS ingest_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source              TEXT,
    started_at          TEXT NOT NULL,
    finished_at         TEXT,
    sessions_added      INTEGER NOT NULL DEFAULT 0,
    sessions_updated    INTEGER NOT NULL DEFAULT 0,
    messages_added      INTEGER NOT NULL DEFAULT 0,
    messages_updated    INTEGER NOT NULL DEFAULT 0,
    error               TEXT
);

CREATE TABLE IF NOT EXISTS sync_state (
    source              TEXT PRIMARY KEY,
    last_scan_at        TEXT,
    last_watermark_mtime REAL
);
