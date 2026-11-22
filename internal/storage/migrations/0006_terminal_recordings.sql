-- Full TTY session recording for interactive terminals. terminal_sessions is
-- the replayable unit (opened per websocket PTY); terminal_events holds the
-- output stream chunks plus the submitted command lines, timestamped relative
-- to session open so the Audit page can play them back with original timing.

CREATE TABLE IF NOT EXISTS terminal_sessions (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    username TEXT NOT NULL DEFAULT '',
    server_id TEXT NOT NULL,
    server_name TEXT NOT NULL DEFAULT '',
    ssh_user TEXT NOT NULL DEFAULT '',
    server_host TEXT NOT NULL DEFAULT '',
    client_ip TEXT NOT NULL DEFAULT '',
    policy_mode TEXT NOT NULL DEFAULT 'audit',
    opened_at TEXT NOT NULL,
    closed_at TEXT,
    close_reason TEXT NOT NULL DEFAULT '',
    byte_size BIGINT NOT NULL DEFAULT 0,
    event_count INTEGER NOT NULL DEFAULT 0,
    truncated INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_terminal_sessions_opened ON terminal_sessions(opened_at DESC);
CREATE INDEX IF NOT EXISTS idx_terminal_sessions_user ON terminal_sessions(user_id);
CREATE INDEX IF NOT EXISTS idx_terminal_sessions_server ON terminal_sessions(server_id);

CREATE TABLE IF NOT EXISTS terminal_events (
    seq BIGSERIAL PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES terminal_sessions(id) ON DELETE CASCADE,
    ts_ms BIGINT NOT NULL,
    direction TEXT NOT NULL DEFAULT 'output',
    data TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_terminal_events_session ON terminal_events(session_id, seq);
