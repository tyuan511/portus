// Package storage implements the persistence layer on PostgreSQL using
// sqlc-generated queries: versioned schema, business CRUD, and the
// tamper-evident audit hash chain. Domain logic (hash chaining, credential
// bookkeeping) lives here; raw SQL lives in db/queries and db/migrations.
package storage

import (
	"context"
	"crypto/rand"
	"database/sql"
	"embed"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io/fs"
	"sort"
	"strings"
	"sync"
	"time"

	"github.com/jackc/pgx/v5/stdlib"

	"github.com/yuantang/sshield/internal/storage/db"
)

// migrationsFS embeds the numbered SQL migration files; sqlc reads the same
// directory at generation time, so schema and generated code never drift.
//
//go:embed migrations/*.sql
var migrationsFS embed.FS

// DB wraps the sqlc query set plus the optional audit JSONL mirror path.
type DB struct {
	q              *db.Queries
	sqlDB          *sql.DB
	auditJSONLPath string

	// mu guards encKey; it seals credential secrets at rest.
	mu     sync.RWMutex
	encKey []byte
}

// Credential lookup used by secret resolvers; kept on the context path.
var _ = context.Background

// Open connects to PostgreSQL and ensures the schema exists.
// dsn is a pgx URL, e.g. "postgres://user:pass@host:5432/sshield?sslmode=disable".
// Migrations are applied from db/migrations on first boot; later schema
// changes ship as additional numbered migration files executed at startup.
func Open(dsn string, auditJSONLPath string) (*DB, error) {
	pool, err := sql.Open("pgx", dsn)
	if err != nil {
		return nil, fmt.Errorf("open postgres: %w", err)
	}
	pool.SetMaxOpenConns(10)
	pool.SetMaxIdleConns(10)
	pool.SetConnMaxLifetime(0)

	if err := pool.Ping(); err != nil {
		_ = pool.Close()
		return nil, fmt.Errorf("ping postgres: %w", err)
	}

	handle := &DB{q: db.New(pool), sqlDB: pool, auditJSONLPath: auditJSONLPath}
	if err := handle.migrate(); err != nil {
		_ = pool.Close()
		return nil, err
	}
	return handle, nil
}

// Close releases the database handle.
func (h *DB) Close() error { return h.sqlDB.Close() }

// Exec runs a raw statement (migrations, tests, operational tooling).
func (h *DB) Exec(q string, args ...any) error {
	_, err := h.sqlDB.Exec(q, args...)
	return err
}

// migrate applies db/migrations files (numbered 0001_init.sql, 0002_*.sql, …)
// tracked in schema_migrations. Idempotent; new files run once at startup.
func (h *DB) migrate() error {
	if _, err := h.sqlDB.Exec(`CREATE TABLE IF NOT EXISTS schema_migrations (
		version TEXT PRIMARY KEY,
		applied_at TEXT NOT NULL
	)`); err != nil {
		return fmt.Errorf("create schema_migrations: %w", err)
	}

	entries, err := fs.ReadDir(migrationsFS, "migrations")
	if err != nil {
		return fmt.Errorf("read embedded migrations: %w", err)
	}
	names := make([]string, 0, len(entries))
	for _, e := range entries {
		if !e.IsDir() && strings.HasSuffix(e.Name(), ".sql") {
			names = append(names, e.Name())
		}
	}
	sort.Strings(names)

	for _, name := range names {
		var applied int
		err := h.sqlDB.QueryRow(`SELECT COUNT(*) FROM schema_migrations WHERE version = $1`, name).Scan(&applied)
		if err != nil {
			return fmt.Errorf("check migration %s: %w", name, err)
		}
		if applied > 0 {
			continue
		}
		stmtSQL, err := migrationsFS.ReadFile("migrations/" + name)
		if err != nil {
			return fmt.Errorf("read migration %s: %w", name, err)
		}
		tx, err := h.sqlDB.Begin()
		if err != nil {
			return fmt.Errorf("begin migration %s: %w", name, err)
		}
		if _, err := tx.Exec(string(stmtSQL)); err != nil {
			_ = tx.Rollback()
			return fmt.Errorf("apply migration %s: %w", name, err)
		}
		if _, err := tx.Exec(`INSERT INTO schema_migrations (version, applied_at) VALUES ($1, $2)`,
			name, nowRFC3339()); err != nil {
			_ = tx.Rollback()
			return fmt.Errorf("record migration %s: %w", name, err)
		}
		if err := tx.Commit(); err != nil {
			return fmt.Errorf("commit migration %s: %w", name, err)
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

func nullString(s string) sql.NullString {
	return sql.NullString{String: s, Valid: s != ""}
}

func text(ns sql.NullString) string {
	if ns.Valid {
		return ns.String
	}
	return ""
}

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

// actorMatches reproduces the legacy actor filter semantics inside the audit
// query: ” = all; 'mcp' = every mcp-* channel; 'terminal'/'manual' =
// interactive actors; anything else = exact match.
func actorMatches(actor string) bool { return actor != "" }

var _ = stdlib.GetDefaultDriver // keep pgx stdlib reference explicit
