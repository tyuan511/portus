-- SSHield Enterprise schema (PostgreSQL).
-- Timestamps are RFC3339 text: lexicographic ordering keeps the audit hash
-- chain and time filters identical across timezones and drivers.

CREATE TABLE IF NOT EXISTS servers (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    host TEXT NOT NULL,
    port INTEGER NOT NULL,
    username TEXT NOT NULL,
    auth_kind TEXT NOT NULL,
    identity_file TEXT NOT NULL DEFAULT '',
    credential_id TEXT NOT NULL DEFAULT '',
    jump_host_id TEXT NOT NULL DEFAULT '',
    group_name TEXT NOT NULL DEFAULT '',
    tags_json TEXT NOT NULL DEFAULT '[]',
    description TEXT NOT NULL DEFAULT '',
    enabled INTEGER NOT NULL DEFAULT 1,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_servers_group ON servers(group_name);

CREATE TABLE IF NOT EXISTS security_rules (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    match_kind TEXT NOT NULL,
    pattern TEXT NOT NULL,
    decision TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    enabled INTEGER NOT NULL DEFAULT 1,
    priority INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS l2_configs (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    protocol TEXT NOT NULL,
    base_url TEXT NOT NULL,
    model TEXT NOT NULL,
    api_key_credential_id TEXT NOT NULL DEFAULT '',
    timeout_ms BIGINT NOT NULL,
    risk_threshold_ask INTEGER NOT NULL,
    risk_threshold_deny INTEGER NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS snippets (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    command TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    tags_json TEXT NOT NULL DEFAULT '[]'
);

CREATE TABLE IF NOT EXISTS app_settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS audit_logs (
    seq BIGSERIAL PRIMARY KEY,
    id TEXT UNIQUE NOT NULL,
    trace_id TEXT NOT NULL DEFAULT '',
    actor TEXT NOT NULL DEFAULT 'mcp',
    timestamp TEXT NOT NULL,
    server_id TEXT NOT NULL,
    server_name TEXT NOT NULL,
    ssh_user TEXT NOT NULL DEFAULT '',
    server_host TEXT NOT NULL DEFAULT '',
    operator TEXT NOT NULL DEFAULT '',
    user_id TEXT NOT NULL DEFAULT '',
    username TEXT NOT NULL DEFAULT '',
    command TEXT NOT NULL,
    intent TEXT NOT NULL,
    dry_run INTEGER NOT NULL DEFAULT 0,
    decision TEXT NOT NULL,
    l1_verdict_json TEXT,
    l2_verdict_json TEXT,
    approval_id TEXT,
    approval_status TEXT,
    exit_code INTEGER,
    duration_ms BIGINT,
    stdout_preview TEXT,
    stderr_preview TEXT,
    error TEXT,
    prev_hash TEXT NOT NULL DEFAULT 'GENESIS',
    event_hash TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_audit_logs_ts ON audit_logs(timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_audit_logs_server ON audit_logs(server_id);
CREATE INDEX IF NOT EXISTS idx_audit_logs_user ON audit_logs(user_id);

CREATE TABLE IF NOT EXISTS roles (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    permissions_json TEXT NOT NULL DEFAULT '[]',
    is_builtin INTEGER NOT NULL DEFAULT 0,
    description TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL DEFAULT '',
    password_hash TEXT NOT NULL,
    role_id TEXT NOT NULL REFERENCES roles(id),
    is_active INTEGER NOT NULL DEFAULT 1,
    must_change_password INTEGER NOT NULL DEFAULT 0,
    totp_secret TEXT NOT NULL DEFAULT '',
    totp_enabled INTEGER NOT NULL DEFAULT 0,
    recovery_codes_json TEXT NOT NULL DEFAULT '[]',
    failed_attempts INTEGER NOT NULL DEFAULT 0,
    locked_until TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_login_at TEXT
);

CREATE TABLE IF NOT EXISTS user_groups (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS user_group_members (
    group_id TEXT NOT NULL REFERENCES user_groups(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY (group_id, user_id)
);
CREATE INDEX IF NOT EXISTS idx_group_members_user ON user_group_members(user_id);

CREATE TABLE IF NOT EXISTS host_grants (
    id TEXT PRIMARY KEY,
    user_group_id TEXT NOT NULL REFERENCES user_groups(id) ON DELETE CASCADE,
    server_id TEXT NOT NULL DEFAULT '',
    group_name TEXT NOT NULL DEFAULT '',
    can_connect INTEGER NOT NULL DEFAULT 0,
    can_execute INTEGER NOT NULL DEFAULT 0,
    created_by TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_host_grants_group ON host_grants(user_group_id);

CREATE TABLE IF NOT EXISTS api_tokens (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    prefix TEXT NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL,
    last_used_at TEXT,
    expires_at TEXT,
    revoked_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_api_tokens_user ON api_tokens(user_id);

CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL UNIQUE,
    ip TEXT NOT NULL DEFAULT '',
    user_agent TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    last_seen_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    absolute_expires_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_sessions_user ON sessions(user_id);

CREATE TABLE IF NOT EXISTS auth_attempts (
    id BIGSERIAL PRIMARY KEY,
    username TEXT NOT NULL DEFAULT '',
    ip TEXT NOT NULL DEFAULT '',
    success INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_auth_attempts_username_time ON auth_attempts(username, created_at);
CREATE INDEX IF NOT EXISTS idx_auth_attempts_ip_time ON auth_attempts(ip, created_at);
