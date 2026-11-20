package storage

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"time"

	"github.com/yuantang/sshield/internal/apierr"
	"github.com/yuantang/sshield/internal/storage/db"
)

// ErrSessionInvalid reports an unknown, foreign, closed or expired session.
var ErrSessionInvalid = errors.New("mcp session invalid or expired")

// HostSession is a live server-issued host authorization for an MCP agent.
type HostSession struct {
	ID        string
	UserID    string
	ServerID  string
	Purpose   string
	CreatedAt time.Time
	ExpiresAt time.Time
}

// CreateHostSession mints a temporary authorization bound to (user, server).
func (h *DB) CreateHostSession(id, userID, serverID, purpose string, ttl time.Duration) (*HostSession, error) {
	now := nowUTC()
	exp := now.Add(ttl)
	err := h.q.CreateMCPSession(context.Background(), db.CreateMCPSessionParams{
		ID:        id,
		UserID:    userID,
		ServerID:  serverID,
		Purpose:   purpose,
		CreatedAt: formatRFC3339(now),
		ExpiresAt: formatRFC3339(exp),
	})
	if err != nil {
		return nil, fmt.Errorf("create mcp session: %w", err)
	}
	return &HostSession{ID: id, UserID: userID, ServerID: serverID, Purpose: purpose, CreatedAt: now, ExpiresAt: exp}, nil
}

// ValidateHostSession checks ownership, closure and expiry, returning the session.
func (h *DB) ValidateHostSession(id, userID string) (*HostSession, error) {
	row, err := h.q.GetMCPSession(context.Background(), id)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, ErrSessionInvalid
	}
	if err != nil {
		return nil, fmt.Errorf("get mcp session: %w", err)
	}
	if row.UserID != userID || row.ClosedAt.Valid {
		return nil, ErrSessionInvalid
	}
	exp, ok := parseRFC3339(row.ExpiresAt)
	if !ok || nowUTC().After(exp) {
		return nil, ErrSessionInvalid
	}
	created, _ := parseRFC3339(row.CreatedAt)
	return &HostSession{ID: row.ID, UserID: row.UserID, ServerID: row.ServerID,
		Purpose: row.Purpose, CreatedAt: created, ExpiresAt: exp}, nil
}

// CloseHostSession marks a session closed (idempotent per caller).
func (h *DB) CloseHostSession(id, userID string) (bool, error) {
	n, err := h.q.CloseMCPSession(context.Background(), db.CloseMCPSessionParams{
		ID:       id,
		UserID:   userID,
		ClosedAt: nullString(nowRFC3339()),
	})
	if err != nil {
		return false, fmt.Errorf("close mcp session: %w", err)
	}
	return n > 0, nil
}

var _ = apierr.CodeUnauthorized
