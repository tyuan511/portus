// Package storage implements the SQLite persistence layer: schema migrations,
// business CRUD, and the tamper-evident audit hash chain.
package storage

import (
	"crypto/rand"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"net/url"
	"os"
	"path/filepath"
	"time"

	_ "modernc.org/sqlite" // pure-Go SQLite driver (database/sql "sqlite")
)

// DB wraps the SQLite handle plus the optional audit JSONL mirror path.
type DB struct {
	sql          *sql.DB
	auditJSONLPath string
}

// Open creates/opens the SQLite database at dbPath and runs pending migrations.
func Open(dbPath string, auditJSONLPath string) (*DB, error) {
	if err := os.MkdirAll(filepath.Dir(dbPath), 0o755); err != nil {
		return nil, fmt.Errorf("create data dir: %w", err)
	}

	dsn := "file:" + url.PathEscape(dbPath) +
		"?_pragma=busy_timeout(5000)" +
		"&_pragma=journal_mode(WAL)" +
		"&_pragma=foreign_keys(1)" +
		"&_pragma=synchronous(NORMAL)"

	pool, err := sql.Open("sqlite", dsn)
	if err != nil {
		return nil, fmt.Errorf("open sqlite: %w", err)
	}
	// SQLite serializes writers; a small pool avoids SQLITE_BUSY churn.
	pool.SetMaxOpenConns(4)
	pool.SetMaxIdleConns(4)
	pool.SetConnMaxLifetime(0)

	db := &DB{sql: pool, auditJSONLPath: auditJSONLPath}
	if err := db.migrate(); err != nil {
		_ = pool.Close()
		return nil, err
	}
	return db, nil
}

// Close releases the database handle.
func (db *DB) Close() error { return db.sql.Close() }

// migrations is the ordered, versioned schema migration list.
var migrations = []struct {
	version int
	name    string
	stmts   []string
}{
	{
		version: 1,
		name:    "core business tables",
		stmts: []string{
			`CREATE TABLE IF NOT EXISTS servers (
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
			)`,
			`CREATE TABLE IF NOT EXISTS security_rules (
				id TEXT PRIMARY KEY,
				name TEXT NOT NULL,
				match_kind TEXT NOT NULL,
				pattern TEXT NOT NULL,
				decision TEXT NOT NULL,
				description TEXT NOT NULL DEFAULT '',
				enabled INTEGER NOT NULL DEFAULT 1,
				priority INTEGER NOT NULL DEFAULT 0
			)`,
			`CREATE TABLE IF NOT EXISTS l2_configs (
				id TEXT PRIMARY KEY,
				name TEXT NOT NULL,
				protocol TEXT NOT NULL,
				base_url TEXT NOT NULL,
				model TEXT NOT NULL,
				api_key_credential_id TEXT NOT NULL DEFAULT '',
				timeout_ms INTEGER NOT NULL,
				risk_threshold_ask INTEGER NOT NULL,
				risk_threshold_deny INTEGER NOT NULL,
				enabled INTEGER NOT NULL DEFAULT 0
			)`,
			`CREATE TABLE IF NOT EXISTS snippets (
				id TEXT PRIMARY KEY,
				name TEXT NOT NULL,
				command TEXT NOT NULL,
				description TEXT NOT NULL DEFAULT '',
				tags_json TEXT NOT NULL DEFAULT '[]'
			)`,
			`CREATE TABLE IF NOT EXISTS app_settings (
				key TEXT PRIMARY KEY,
				value TEXT NOT NULL,
				updated_at TEXT NOT NULL
			)`,
			`CREATE INDEX IF NOT EXISTS idx_servers_group ON servers(group_name)`,
		},
	},
	{
		version: 2,
		name:    "audit log with hash chain and multi-user columns",
		stmts: []string{
			`CREATE TABLE IF NOT EXISTS audit_logs (
				seq INTEGER PRIMARY KEY AUTOINCREMENT,
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
				duration_ms INTEGER,
				stdout_preview TEXT,
				stderr_preview TEXT,
				error TEXT,
				prev_hash TEXT NOT NULL DEFAULT 'GENESIS',
				event_hash TEXT NOT NULL DEFAULT ''
			)`,
			`CREATE INDEX IF NOT EXISTS idx_audit_logs_ts ON audit_logs(timestamp DESC)`,
			`CREATE INDEX IF NOT EXISTS idx_audit_logs_server ON audit_logs(server_id)`,
			`CREATE INDEX IF NOT EXISTS idx_audit_logs_user ON audit_logs(user_id)`,
		},
	},
	{
		version: 3,
		name:    "enterprise RBAC tables",
		stmts: []string{
			`CREATE TABLE IF NOT EXISTS roles (
				id TEXT PRIMARY KEY,
				name TEXT NOT NULL UNIQUE,
				permissions_json TEXT NOT NULL DEFAULT '[]',
				is_builtin INTEGER NOT NULL DEFAULT 0,
				description TEXT NOT NULL DEFAULT '',
				created_at TEXT NOT NULL
			)`,
			`CREATE TABLE IF NOT EXISTS users (
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
			)`,
			`CREATE TABLE IF NOT EXISTS user_groups (
				id TEXT PRIMARY KEY,
				name TEXT NOT NULL UNIQUE,
				description TEXT NOT NULL DEFAULT '',
				created_at TEXT NOT NULL
			)`,
			`CREATE TABLE IF NOT EXISTS user_group_members (
				group_id TEXT NOT NULL REFERENCES user_groups(id) ON DELETE CASCADE,
				user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
				PRIMARY KEY (group_id, user_id)
			)`,
			`CREATE TABLE IF NOT EXISTS host_grants (
				id TEXT PRIMARY KEY,
				user_group_id TEXT NOT NULL REFERENCES user_groups(id) ON DELETE CASCADE,
				server_id TEXT NOT NULL DEFAULT '',
				group_name TEXT NOT NULL DEFAULT '',
				can_connect INTEGER NOT NULL DEFAULT 0,
				can_execute INTEGER NOT NULL DEFAULT 0,
				created_by TEXT NOT NULL DEFAULT '',
				created_at TEXT NOT NULL
			)`,
			`CREATE INDEX IF NOT EXISTS idx_host_grants_group ON host_grants(user_group_id)`,
			`CREATE INDEX IF NOT EXISTS idx_group_members_user ON user_group_members(user_id)`,
		},
	},
	{
		version: 4,
		name:    "api tokens, sessions and auth attempts",
		stmts: []string{
			`CREATE TABLE IF NOT EXISTS api_tokens (
				id TEXT PRIMARY KEY,
				user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
				name TEXT NOT NULL,
				prefix TEXT NOT NULL,
				token_hash TEXT NOT NULL UNIQUE,
				created_at TEXT NOT NULL,
				last_used_at TEXT,
				expires_at TEXT,
				revoked_at TEXT
			)`,
			`CREATE INDEX IF NOT EXISTS idx_api_tokens_user ON api_tokens(user_id)`,
			`CREATE TABLE IF NOT EXISTS sessions (
				id TEXT PRIMARY KEY,
				user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
				token_hash TEXT NOT NULL UNIQUE,
				ip TEXT NOT NULL DEFAULT '',
				user_agent TEXT NOT NULL DEFAULT '',
				created_at TEXT NOT NULL,
				last_seen_at TEXT NOT NULL,
				expires_at TEXT NOT NULL,
				absolute_expires_at TEXT NOT NULL
			)`,
			`CREATE INDEX IF NOT EXISTS idx_sessions_user ON sessions(user_id)`,
			`CREATE TABLE IF NOT EXISTS auth_attempts (
				id INTEGER PRIMARY KEY AUTOINCREMENT,
				username TEXT NOT NULL DEFAULT '',
				ip TEXT NOT NULL DEFAULT '',
				success INTEGER NOT NULL DEFAULT 0,
				created_at TEXT NOT NULL
			)`,
			`CREATE INDEX IF NOT EXISTS idx_auth_attempts_username_time ON auth_attempts(username, created_at)`,
			`CREATE INDEX IF NOT EXISTS idx_auth_attempts_ip_time ON auth_attempts(ip, created_at)`,
		},
	},
}

func (db *DB) migrate() error {
	if _, err := db.sql.Exec(`CREATE TABLE IF NOT EXISTS schema_migrations (
		version INTEGER PRIMARY KEY,
		name TEXT NOT NULL,
		applied_at TEXT NOT NULL
	)`); err != nil {
		return fmt.Errorf("create schema_migrations: %w", err)
	}

	for _, m := range migrations {
		var applied int
		err := db.sql.QueryRow(`SELECT version FROM schema_migrations WHERE version = ?`, m.version).Scan(&applied)
		if err == nil {
			continue // already applied
		}
		if err != sql.ErrNoRows {
			return fmt.Errorf("check migration %d: %w", m.version, err)
		}
		tx, err := db.sql.Begin()
		if err != nil {
			return fmt.Errorf("begin migration %d: %w", m.version, err)
		}
		for _, stmt := range m.stmts {
			if _, err := tx.Exec(stmt); err != nil {
				_ = tx.Rollback()
				return fmt.Errorf("migration %d (%s): %w", m.version, m.name, err)
			}
		}
		if _, err := tx.Exec(`INSERT INTO schema_migrations (version, name, applied_at) VALUES (?, ?, ?)`,
			m.version, m.name, nowRFC3339()); err != nil {
			_ = tx.Rollback()
			return fmt.Errorf("record migration %d: %w", m.version, err)
		}
		if err := tx.Commit(); err != nil {
			return fmt.Errorf("commit migration %d: %w", m.version, err)
		}
	}
	return nil
}

func nowRFC3339() string { return time.Now().UTC().Format(time.RFC3339Nano) }

func nowUTC() time.Time { return time.Now().UTC() }

func mustParse(s string) time.Time {
	t, ok := parseRFC3339(s)
	if !ok {
		return time.Time{}
	}
	return t
}

// shortID returns 8 hex chars of crypto randomness for entity ids.
func shortID() string {
	var b [4]byte
	if _, err := rand.Read(b[:]); err != nil {
		panic("crypto/rand unavailable: " + err.Error())
	}
	return hex.EncodeToString(b[:])
}

func formatRFC3339(t time.Time) string { return t.UTC().Format(time.RFC3339Nano) }

func parseRFC3339(s string) (time.Time, bool) {
	t, err := time.Parse(time.RFC3339Nano, s)
	if err != nil {
		return time.Time{}, false
	}
	return t, true
}

func boolToInt(b bool) int {
	if b {
		return 1
	}
	return 0
}

func intToBool(i int) bool { return i != 0 }

func nullString(s string) any {
	if s == "" {
		return nil
	}
	return s
}

// idLike reproduces the legacy actor filter semantics: "mcp" matches every
// mcp-* channel, "terminal"/"manual" match interactive actors.
func actorFilter(actor string) (string, any) {
	switch actor {
	case "mcp":
		return "actor LIKE 'mcp%'", nil
	case "terminal", "manual":
		return "actor IN ('terminal','manual')", nil
	default:
		return "actor = ?", actor
	}
}

// tagsJSON marshals a tag slice to the JSON stored in TEXT columns.
func tagsJSON(tags []string) string {
	if len(tags) == 0 {
		return "[]"
	}
	b, err := json.Marshal(tags)
	if err != nil {
		return "[]"
	}
	return string(b)
}

func parseTags(s string) []string {
	var tags []string
	if s == "" || s == "[]" {
		return []string{}
	}
	if err := json.Unmarshal([]byte(s), &tags); err != nil || tags == nil {
		return []string{}
	}
	return tags
}
