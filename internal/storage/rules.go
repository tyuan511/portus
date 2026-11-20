package storage

import (
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"

	"github.com/yuantang/sshield/internal/domain"
)

// --- security rules ---

const ruleCols = `id, name, match_kind, pattern, decision, description, enabled, priority`

func scanRule(row interface{ Scan(...any) error }) (*domain.SecurityRule, error) {
	var r domain.SecurityRule
	var enabled int
	if err := row.Scan(&r.ID, &r.Name, &r.MatchKind, &r.Pattern, &r.Decision,
		&r.Description, &enabled, &r.Priority); err != nil {
		return nil, err
	}
	r.Enabled = intToBool(enabled)
	return &r, nil
}

// UpsertRule inserts or updates an L1 security rule.
func (db *DB) UpsertRule(r *domain.SecurityRule) error {
	_, err := db.sql.Exec(`INSERT INTO security_rules (`+ruleCols+`) VALUES (?,?,?,?,?,?,?,?)
		ON CONFLICT(id) DO UPDATE SET name=excluded.name, match_kind=excluded.match_kind,
		pattern=excluded.pattern, decision=excluded.decision, description=excluded.description,
		enabled=excluded.enabled, priority=excluded.priority`,
		r.ID, r.Name, string(r.MatchKind), r.Pattern, string(r.Decision),
		r.Description, boolToInt(r.Enabled), r.Priority)
	if err != nil {
		return fmt.Errorf("upsert rule: %w", err)
	}
	return nil
}

// ListRules returns all security rules ordered by priority (desc).
func (db *DB) ListRules() ([]*domain.SecurityRule, error) {
	rows, err := db.sql.Query(`SELECT ` + ruleCols + ` FROM security_rules ORDER BY priority DESC`)
	if err != nil {
		return nil, fmt.Errorf("list rules: %w", err)
	}
	defer rows.Close()
	var out []*domain.SecurityRule
	for rows.Next() {
		r, err := scanRule(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, r)
	}
	return out, rows.Err()
}

// DeleteRule removes a rule; returns whether a row was deleted.
func (db *DB) DeleteRule(id string) (bool, error) {
	return db.execDelete(`DELETE FROM security_rules WHERE id = ?`, id)
}

// CountRules returns the number of stored rules (seed detection).
func (db *DB) CountRules() (int, error) {
	var n int
	err := db.sql.QueryRow(`SELECT COUNT(*) FROM security_rules`).Scan(&n)
	return n, err
}

// --- L2 config ---

// GetL2Config returns the active L2 provider configuration.
func (db *DB) GetL2Config() (*domain.L2ProviderConfig, error) {
	row := db.sql.QueryRow(`SELECT id, name, protocol, base_url, model, api_key_credential_id,
		timeout_ms, risk_threshold_ask, risk_threshold_deny, enabled FROM l2_configs ORDER BY id LIMIT 1`)
	var c domain.L2ProviderConfig
	var enabled int
	err := row.Scan(&c.ID, &c.Name, &c.Protocol, &c.BaseURL, &c.Model, &c.APIKeyCredentialID,
		&c.TimeoutMs, &c.RiskThresholdAsk, &c.RiskThresholdDeny, &enabled)
	if errors.Is(err, sql.ErrNoRows) {
		def := domain.DefaultL2ProviderConfig()
		return &def, nil
	}
	if err != nil {
		return nil, fmt.Errorf("get l2 config: %w", err)
	}
	c.Enabled = intToBool(enabled)
	return &c, nil
}

// UpsertL2Config saves the L2 provider configuration.
func (db *DB) UpsertL2Config(c *domain.L2ProviderConfig) error {
	_, err := db.sql.Exec(`INSERT INTO l2_configs (id, name, protocol, base_url, model,
		api_key_credential_id, timeout_ms, risk_threshold_ask, risk_threshold_deny, enabled)
		VALUES (?,?,?,?,?,?,?,?,?,?)
		ON CONFLICT(id) DO UPDATE SET name=excluded.name, protocol=excluded.protocol,
		base_url=excluded.base_url, model=excluded.model,
		api_key_credential_id=excluded.api_key_credential_id, timeout_ms=excluded.timeout_ms,
		risk_threshold_ask=excluded.risk_threshold_ask,
		risk_threshold_deny=excluded.risk_threshold_deny, enabled=excluded.enabled`,
		c.ID, c.Name, string(c.Protocol), c.BaseURL, c.Model, c.APIKeyCredentialID,
		c.TimeoutMs, c.RiskThresholdAsk, c.RiskThresholdDeny, boolToInt(c.Enabled))
	if err != nil {
		return fmt.Errorf("upsert l2 config: %w", err)
	}
	return nil
}

// --- snippets ---

// UpsertSnippet inserts or updates a command snippet.
func (db *DB) UpsertSnippet(s *domain.CommandSnippet) error {
	if s.Tags == nil {
		s.Tags = []string{}
	}
	_, err := db.sql.Exec(`INSERT INTO snippets (id, name, command, description, tags_json)
		VALUES (?,?,?,?,?)
		ON CONFLICT(id) DO UPDATE SET name=excluded.name, command=excluded.command,
		description=excluded.description, tags_json=excluded.tags_json`,
		s.ID, s.Name, s.Command, s.Description, tagsJSON(s.Tags))
	if err != nil {
		return fmt.Errorf("upsert snippet: %w", err)
	}
	return nil
}

// ListSnippets returns all snippets.
func (db *DB) ListSnippets() ([]*domain.CommandSnippet, error) {
	rows, err := db.sql.Query(`SELECT id, name, command, description, tags_json FROM snippets ORDER BY name`)
	if err != nil {
		return nil, fmt.Errorf("list snippets: %w", err)
	}
	defer rows.Close()
	var out []*domain.CommandSnippet
	for rows.Next() {
		var s domain.CommandSnippet
		var tagsJSONStr string
		if err := rows.Scan(&s.ID, &s.Name, &s.Command, &s.Description, &tagsJSONStr); err != nil {
			return nil, err
		}
		s.Tags = parseTags(tagsJSONStr)
		out = append(out, &s)
	}
	return out, rows.Err()
}

// DeleteSnippet removes a snippet; returns whether a row was deleted.
func (db *DB) DeleteSnippet(id string) (bool, error) {
	return db.execDelete(`DELETE FROM snippets WHERE id = ?`, id)
}

// --- settings KV ---

// GetSetting returns a global setting value.
func (db *DB) GetSetting(key string) (string, bool, error) {
	var v string
	err := db.sql.QueryRow(`SELECT value FROM app_settings WHERE key = ?`, key).Scan(&v)
	if errors.Is(err, sql.ErrNoRows) {
		return "", false, nil
	}
	if err != nil {
		return "", false, fmt.Errorf("get setting: %w", err)
	}
	return v, true, nil
}

// SetSetting writes a global setting value.
func (db *DB) SetSetting(key, value string) error {
	_, err := db.sql.Exec(`INSERT INTO app_settings (key, value, updated_at) VALUES (?,?,?)
		ON CONFLICT(key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at`,
		key, value, nowRFC3339())
	if err != nil {
		return fmt.Errorf("set setting: %w", err)
	}
	return nil
}

// GetJSONSetting reads a JSON-encoded setting into out; ok=false when absent.
func (db *DB) GetJSONSetting(key string, out any) (bool, error) {
	v, ok, err := db.GetSetting(key)
	if err != nil || !ok {
		return false, err
	}
	if err := json.Unmarshal([]byte(v), out); err != nil {
		return true, fmt.Errorf("decode setting %s: %w", key, err)
	}
	return true, nil
}

func (db *DB) execDelete(q string, args ...any) (bool, error) {
	res, err := db.sql.Exec(q, args...)
	if err != nil {
		return false, fmt.Errorf("delete: %w", err)
	}
	n, _ := res.RowsAffected()
	return n > 0, nil
}
