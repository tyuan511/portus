package storage

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"time"

	"github.com/tyuan511/bulwark/internal/domain"
	"github.com/tyuan511/bulwark/internal/storage/db"
)

// CreateTerminalSession records the opening of a PTY session.
func (h *DB) CreateTerminalSession(s *domain.TerminalSession) error {
	if s.OpenedAt.IsZero() {
		s.OpenedAt = nowUTC()
	}
	err := h.q.InsertTerminalSession(context.Background(), db.InsertTerminalSessionParams{
		ID:         s.ID,
		UserID:     s.UserID,
		Username:   s.Username,
		ServerID:   s.ServerID,
		ServerName: s.ServerName,
		SshUser:    s.SSHUser,
		ServerHost: s.ServerHost,
		ClientIp:   s.ClientIP,
		PolicyMode: s.PolicyMode,
		OpenedAt:   formatRFC3339(s.OpenedAt),
	})
	if err != nil {
		return fmt.Errorf("create terminal session: %w", err)
	}
	return nil
}

// AppendTerminalEvent persists one recording chunk.
func (h *DB) AppendTerminalEvent(sessionID string, tsMs int64, direction, data string) error {
	err := h.q.AppendTerminalEvent(context.Background(), db.AppendTerminalEventParams{
		SessionID: sessionID,
		TsMs:      tsMs,
		Direction: direction,
		Data:      data,
	})
	if err != nil {
		return fmt.Errorf("append terminal event: %w", err)
	}
	return nil
}

// CloseTerminalSession finalizes counters for a finished recording.
func (h *DB) CloseTerminalSession(id, reason string, closedAt time.Time, byteSize int64, eventCount int64, truncated bool) error {
	n, err := h.q.CloseTerminalSession(context.Background(), db.CloseTerminalSessionParams{
		ID:          id,
		ClosedAt:    nullString(formatRFC3339(closedAt)),
		CloseReason: reason,
		ByteSize:    byteSize,
		EventCount:  int32(eventCount),
		Truncated:   int32(boolToInt(truncated)),
	})
	if err != nil {
		return fmt.Errorf("close terminal session: %w", err)
	}
	if n == 0 {
		return ErrNotFound
	}
	return nil
}

// GetTerminalSession fetches one recording header.
func (h *DB) GetTerminalSession(id string) (*domain.TerminalSession, error) {
	row, err := h.q.GetTerminalSession(context.Background(), id)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, fmt.Errorf("get terminal session: %w", err)
	}
	return terminalSessionToDomain(row), nil
}

// ListTerminalSessions returns recording headers with a total count.
func (h *DB) ListTerminalSessions(q *domain.TerminalSessionQuery) ([]*domain.TerminalSession, int64, error) {
	limit := int64(q.Limit)
	if limit <= 0 {
		limit = 100
	}
	if limit > 500 {
		limit = 500
	}
	params := db.ListTerminalSessionsParams{
		UserID:   nullString(q.UserID),
		ServerID: nullString(q.ServerID),
		Username: nullString(q.Username),
		Lim:      nullInt64(limit),
		Offs:     nullInt64(int64(q.Offset)),
	}
	rows, err := h.q.ListTerminalSessions(context.Background(), params)
	if err != nil {
		return nil, 0, fmt.Errorf("list terminal sessions: %w", err)
	}
	total, err := h.q.CountTerminalSessions(context.Background(), db.CountTerminalSessionsParams{
		UserID:   params.UserID,
		ServerID: params.ServerID,
		Username: params.Username,
	})
	if err != nil {
		return nil, 0, fmt.Errorf("count terminal sessions: %w", err)
	}
	out := make([]*domain.TerminalSession, 0, len(rows))
	for _, r := range rows {
		out = append(out, terminalSessionToDomain(r))
	}
	return out, total, nil
}

// ListTerminalEvents returns the ordered replay stream of one session.
func (h *DB) ListTerminalEvents(sessionID string) ([]*domain.TerminalEvent, error) {
	rows, err := h.q.SelectTerminalEvents(context.Background(), sessionID)
	if err != nil {
		return nil, fmt.Errorf("terminal events: %w", err)
	}
	out := make([]*domain.TerminalEvent, 0, len(rows))
	for _, r := range rows {
		out = append(out, &domain.TerminalEvent{
			Seq:       r.Seq,
			TsMs:      r.TsMs,
			Direction: r.Direction,
			Data:      r.Data,
		})
	}
	return out, nil
}

// CleanupTerminalSessionsBefore prunes recordings older than the cutoff;
// events cascade with their session.
func (h *DB) CleanupTerminalSessionsBefore(cutoff time.Time) (int64, error) {
	n, err := h.q.CleanupTerminalSessionsBefore(context.Background(), formatRFC3339(cutoff))
	if err != nil {
		return 0, fmt.Errorf("cleanup terminal sessions: %w", err)
	}
	return n, nil
}

// CleanupApiTokensBefore prunes revoked/expired tokens older than the cutoff.
func (h *DB) CleanupApiTokensBefore(cutoff time.Time) (int64, error) {
	n, err := h.q.CleanupApiTokensBefore(context.Background(), nullString(formatRFC3339(cutoff)))
	if err != nil {
		return 0, fmt.Errorf("cleanup api tokens: %w", err)
	}
	return n, nil
}

// CleanupMCPSessionsBefore prunes the MCP host-session registry (audit
// events referenced by sessions are part of the hash chain and stay).
func (h *DB) CleanupMCPSessionsBefore(cutoff time.Time) (int64, error) {
	n, err := h.q.CleanupMCPSessionsBefore(context.Background(), formatRFC3339(cutoff))
	if err != nil {
		return 0, fmt.Errorf("cleanup mcp sessions: %w", err)
	}
	return n, nil
}

func terminalSessionToDomain(r db.TerminalSession) *domain.TerminalSession {
	s := &domain.TerminalSession{
		ID:          r.ID,
		UserID:      r.UserID,
		Username:    r.Username,
		ServerID:    r.ServerID,
		ServerName:  r.ServerName,
		SSHUser:     r.SshUser,
		ServerHost:  r.ServerHost,
		ClientIP:    r.ClientIp,
		PolicyMode:  r.PolicyMode,
		CloseReason: r.CloseReason,
		ByteSize:    r.ByteSize,
		EventCount:  int64(r.EventCount),
		Truncated:   r.Truncated == 1,
	}
	s.OpenedAt, _ = parseRFC3339(r.OpenedAt)
	if r.ClosedAt.Valid {
		if t, ok := parseRFC3339(r.ClosedAt.String); ok {
			s.ClosedAt = &t
		}
	}
	return s
}
