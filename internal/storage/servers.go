package storage

import (
	"database/sql"
	"errors"
	"fmt"

	"github.com/yuantang/sshield/internal/domain"
)

// ErrNotFound is returned when a queried row does not exist.
var ErrNotFound = errors.New("not found")

const serverCols = `id, name, host, port, username, auth_kind, identity_file, credential_id,
	jump_host_id, group_name, tags_json, description, enabled, updated_at`

func scanServer(row interface{ Scan(...any) error }) (*domain.ServerAsset, error) {
	var a domain.ServerAsset
	var tagsJSON string
	var enabled int
	if err := row.Scan(&a.ID, &a.Name, &a.Host, &a.Port, &a.Username, &a.AuthKind,
		&a.IdentityFile, &a.CredentialID, &a.JumpHostID, &a.GroupName,
		&tagsJSON, &a.Description, &enabled, new(string)); err != nil {
		return nil, err
	}
	a.Tags = parseTags(tagsJSON)
	a.Enabled = intToBool(enabled)
	return &a, nil
}

// UpsertServer inserts or updates a server asset.
func (db *DB) UpsertServer(a *domain.ServerAsset) error {
	if a.Tags == nil {
		a.Tags = []string{}
	}
	if a.AuthKind == "" {
		a.AuthKind = domain.AuthAgentOrDefaultKey
	}
	_, err := db.sql.Exec(`INSERT INTO servers (`+serverCols+`) VALUES
		(?,?,?,?,?,?,?,?,?,?,?,?,?,?)
		ON CONFLICT(id) DO UPDATE SET
			name=excluded.name, host=excluded.host, port=excluded.port, username=excluded.username,
			auth_kind=excluded.auth_kind, identity_file=excluded.identity_file,
			credential_id=excluded.credential_id, jump_host_id=excluded.jump_host_id,
			group_name=excluded.group_name, tags_json=excluded.tags_json,
			description=excluded.description, enabled=excluded.enabled,
			updated_at=excluded.updated_at`,
		a.ID, a.Name, a.Host, a.Port, a.Username, string(a.AuthKind),
		a.IdentityFile, a.CredentialID, a.JumpHostID, a.GroupName,
		tagsJSON(a.Tags), a.Description, boolToInt(a.Enabled), nowRFC3339())
	if err != nil {
		return fmt.Errorf("upsert server: %w", err)
	}
	return nil
}

// GetServer fetches one server asset by id.
func (db *DB) GetServer(id string) (*domain.ServerAsset, error) {
	row := db.sql.QueryRow(`SELECT `+serverCols+` FROM servers WHERE id = ?`, id)
	a, err := scanServer(row)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, ErrNotFound
	}
	return a, err
}

// ListServers returns every server asset (admin/full view).
func (db *DB) ListServers() ([]*domain.ServerAsset, error) {
	return db.queryServers(`SELECT ` + serverCols + ` FROM servers ORDER BY group_name, name`)
}

// ListEnabledServers returns enabled assets only (MCP consumption).
func (db *DB) ListEnabledServers() ([]*domain.ServerAsset, error) {
	return db.queryServers(`SELECT ` + serverCols + ` FROM servers WHERE enabled = 1 ORDER BY group_name, name`)
}

// ListServersForUser returns enabled servers the user is granted access to,
// honoring per-host and host-group grants across all of the user's groups.
func (db *DB) ListServersForUser(userID string) ([]*domain.ServerAsset, error) {
	return db.queryServers(`SELECT `+serverCols+` FROM servers s WHERE s.enabled = 1 AND EXISTS (
		SELECT 1 FROM host_grants g
		JOIN user_group_members m ON m.group_id = g.user_group_id
		WHERE m.user_id = ?
		  AND ((g.server_id != '' AND g.server_id = s.id)
		    OR (g.group_name != '' AND g.group_name = s.group_name))
	) ORDER BY s.group_name, s.name`, userID)
}

// ListServersForUserFiltered additionally requires can_connect or can_execute
// on the matching grant (used when the consumer only executes or only connects).
func (db *DB) ListServersForUserFiltered(userID string, needExecute bool) ([]*domain.ServerAsset, error) {
	col := "g.can_connect"
	if needExecute {
		col = "g.can_execute"
	}
	return db.queryServers(`SELECT `+serverCols+` FROM servers s WHERE s.enabled = 1 AND EXISTS (
		SELECT 1 FROM host_grants g
		JOIN user_group_members m ON m.group_id = g.user_group_id
		WHERE m.user_id = ? AND `+col+` = 1
		  AND ((g.server_id != '' AND g.server_id = s.id)
		    OR (g.group_name != '' AND g.group_name = s.group_name))
	) ORDER BY s.group_name, s.name`, userID)
}

// ServerGrantDecision evaluates a user's effective access on one server.
func (db *DB) ServerGrantDecision(userID string, asset *domain.ServerAsset) (domain.GrantDecision, error) {
	var conn, exec sql.NullInt64
	err := db.sql.QueryRow(`SELECT
			MAX(g.can_connect), MAX(g.can_execute)
		FROM host_grants g
		JOIN user_group_members m ON m.group_id = g.user_group_id
		WHERE m.user_id = ?
		  AND ((g.server_id != '' AND g.server_id = ?)
		    OR (g.group_name != '' AND g.group_name = ?))`,
		userID, asset.ID, asset.GroupName,
	).Scan(&conn, &exec)
	if err != nil && !errors.Is(err, sql.ErrNoRows) {
		return domain.GrantDecision{}, fmt.Errorf("grant decision: %w", err)
	}
	d := domain.GrantDecision{
		Allowed:    (conn.Valid && conn.Int64 == 1) || (exec.Valid && exec.Int64 == 1),
		CanConnect: conn.Valid && conn.Int64 == 1,
		CanExecute: exec.Valid && exec.Int64 == 1,
	}
	return d, nil
}

// DeleteServer removes a server asset; returns whether a row was deleted.
func (db *DB) DeleteServer(id string) (bool, error) {
	res, err := db.sql.Exec(`DELETE FROM servers WHERE id = ?`, id)
	if err != nil {
		return false, fmt.Errorf("delete server: %w", err)
	}
	n, _ := res.RowsAffected()
	return n > 0, nil
}

// DistinctServerGroups lists the existing host-group names.
func (db *DB) DistinctServerGroups() ([]string, error) {
	rows, err := db.sql.Query(`SELECT DISTINCT group_name FROM servers WHERE group_name != '' ORDER BY group_name`)
	if err != nil {
		return nil, fmt.Errorf("list groups: %w", err)
	}
	defer rows.Close()
	var out []string
	for rows.Next() {
		var g string
		if err := rows.Scan(&g); err != nil {
			return nil, err
		}
		out = append(out, g)
	}
	return out, rows.Err()
}

func (db *DB) queryServers(q string, args ...any) ([]*domain.ServerAsset, error) {
	rows, err := db.sql.Query(q, args...)
	if err != nil {
		return nil, fmt.Errorf("query servers: %w", err)
	}
	defer rows.Close()
	var out []*domain.ServerAsset
	for rows.Next() {
		a, err := scanServer(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, a)
	}
	return out, rows.Err()
}

// GenerateID produces a short prefixed identifier (e.g. "srv-a1b2c3d4").
func GenerateID(prefix string) string {
	return prefix + "-" + shortID()
}
