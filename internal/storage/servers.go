package storage

import (
	"context"
	"database/sql"
	"errors"
	"fmt"

	"github.com/tyuan511/bulwark/internal/domain"
	"github.com/tyuan511/bulwark/internal/storage/db"
)

// ErrNotFound is returned when a queried row does not exist.
var ErrNotFound = errors.New("not found")

func serverToDomain(s db.Server) *domain.ServerAsset {
	return &domain.ServerAsset{
		ID:                 s.ID,
		Name:               s.Name,
		Host:               s.Host,
		Port:               int(s.Port),
		Username:           s.Username,
		AuthKind:           domain.AuthKind(s.AuthKind),
		IdentityFile:       s.IdentityFile,
		CredentialID:       s.CredentialID,
		JumpHostID:         s.JumpHostID,
		JumpMode:           domain.JumpMode(s.JumpMode),
		JumpSuUsername:     s.JumpSuUsername,
		JumpSuCredentialID: s.JumpSuCredentialID,
		GroupName:          s.GroupName,
		Tags:               parseTags(s.TagsJson),
		Description:        s.Description,
		Enabled:            s.Enabled == 1,
	}
}

func serverParams(a *domain.ServerAsset) db.UpsertServerParams {
	if a.Tags == nil {
		a.Tags = []string{}
	}
	if a.AuthKind == "" {
		a.AuthKind = domain.AuthAgentOrDefaultKey
	}
	return db.UpsertServerParams{
		ID:                 a.ID,
		Name:               a.Name,
		Host:               a.Host,
		Port:               int32(a.Port),
		Username:           a.Username,
		AuthKind:           string(a.AuthKind),
		IdentityFile:       a.IdentityFile,
		CredentialID:       a.CredentialID,
		JumpHostID:         a.JumpHostID,
		JumpMode:           string(a.JumpMode),
		JumpSuUsername:     a.JumpSuUsername,
		JumpSuCredentialID: a.JumpSuCredentialID,
		GroupName:          a.GroupName,
		TagsJson:           tagsJSON(a.Tags),
		Description:        a.Description,
		Enabled:            int32(boolToInt(a.Enabled)),
		UpdatedAt:          nowRFC3339(),
	}
}

// UpsertServer inserts or updates a server asset.
func (h *DB) UpsertServer(a *domain.ServerAsset) error {
	if err := h.q.UpsertServer(context.Background(), serverParams(a)); err != nil {
		return fmt.Errorf("upsert server: %w", err)
	}
	return nil
}

// GetServer fetches one server asset by id.
func (h *DB) GetServer(id string) (*domain.ServerAsset, error) {
	s, err := h.q.GetServer(context.Background(), id)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, fmt.Errorf("get server: %w", err)
	}
	return serverToDomain(s), nil
}

// ListServers returns every server asset (admin/full view).
func (h *DB) ListServers() ([]*domain.ServerAsset, error) {
	rows, err := h.q.ListServers(context.Background())
	if err != nil {
		return nil, fmt.Errorf("list servers: %w", err)
	}
	out := make([]*domain.ServerAsset, 0, len(rows))
	for _, s := range rows {
		out = append(out, serverToDomain(s))
	}
	return out, nil
}

// ListEnabledServers returns enabled assets only (MCP consumption).
func (h *DB) ListEnabledServers() ([]*domain.ServerAsset, error) {
	rows, err := h.q.ListEnabledServers(context.Background())
	if err != nil {
		return nil, fmt.Errorf("list enabled servers: %w", err)
	}
	out := make([]*domain.ServerAsset, 0, len(rows))
	for _, s := range rows {
		out = append(out, serverToDomain(s))
	}
	return out, nil
}


// ListServersForUserFiltered additionally requires can_connect (needExecute=false)
// or can_execute (needExecute=true) on the matching grant.
func (h *DB) ListServersForUserFiltered(userID string, needExecute bool) ([]*domain.ServerAsset, error) {
	need := int64(0)
	if needExecute {
		need = 1
	}
	rows, err := h.q.ListServersForUserFiltered(context.Background(), db.ListServersForUserFilteredParams{
		UserID:      userID,
		NeedExecute: int32(need),
	})
	if err != nil {
		return nil, fmt.Errorf("list filtered servers: %w", err)
	}
	out := make([]*domain.ServerAsset, 0, len(rows))
	for _, s := range rows {
		out = append(out, serverToDomain(s))
	}
	return out, nil
}

// ServerGrantDecision evaluates a user's effective access on one server.
func (h *DB) ServerGrantDecision(userID string, asset *domain.ServerAsset) (domain.GrantDecision, error) {
	row, err := h.q.ServerGrantDecision(context.Background(), db.ServerGrantDecisionParams{
		UserID:   userID,
		ServerID: asset.ID,
	})
	if err != nil {
		return domain.GrantDecision{}, fmt.Errorf("grant decision: %w", err)
	}
	return domain.GrantDecision{
		Allowed:            row.CanConnect == 1 || row.CanExecute == 1 || row.CanTransfer == 1,
		CanConnect:         row.CanConnect == 1,
		CanExecute:         row.CanExecute == 1,
		CanTransfer:        row.CanTransfer == 1,
		TerminalNoApproval: row.TerminalNoApproval == 1,
		MCPNoApproval:      row.McpNoApproval == 1,
	}, nil
}

// ListServersForUserTransferable returns enabled hosts the user may run
// SFTP file operations on (can_transfer grant bit).
func (h *DB) ListServersForUserTransferable(userID string) ([]*domain.ServerAsset, error) {
	rows, err := h.q.ListServersForUserTransferable(context.Background(), userID)
	if err != nil {
		return nil, fmt.Errorf("list transferable servers: %w", err)
	}
	out := make([]*domain.ServerAsset, 0, len(rows))
	for _, s := range rows {
		out = append(out, serverToDomain(s))
	}
	return out, nil
}

// DeleteServer removes a server asset; returns whether a row was deleted.
func (h *DB) DeleteServer(id string) (bool, error) {
	n, err := h.q.DeleteServer(context.Background(), id)
	if err != nil {
		return false, fmt.Errorf("delete server: %w", err)
	}
	return n > 0, nil
}

// DistinctServerGroups lists the existing host-group names.
func (h *DB) DistinctServerGroups() ([]string, error) {
	rows, err := h.q.DistinctServerGroups(context.Background())
	if err != nil {
		return nil, fmt.Errorf("list groups: %w", err)
	}
	return rows, nil
}
