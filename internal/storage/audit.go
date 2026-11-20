package storage

import (
	"context"
	"crypto/sha256"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"

	"github.com/yuantang/sshield/internal/domain"
	"github.com/yuantang/sshield/internal/storage/db"
)

// GenesisHash is the prev_hash of the first audit record.
const GenesisHash = "GENESIS"

// AppendAudit inserts an event into the hash chain (and optional JSONL mirror).
func (h *DB) AppendAudit(ev *domain.AuditEvent) error {
	if ev.TraceID == "" {
		ev.TraceID = ev.ID
	}
	if ev.Timestamp.IsZero() {
		ev.Timestamp = nowUTC()
	}
	tsStr := formatRFC3339(ev.Timestamp)

	var prevHash string
	tail, err := h.q.GetAuditTailHash(context.Background())
	switch {
	case errors.Is(err, sql.ErrNoRows):
		prevHash = GenesisHash
	case err != nil:
		return fmt.Errorf("audit tail: %w", err)
	default:
		prevHash = tail
	}

	ev.PrevHash = prevHash
	ev.EventHash = computeEventHash(ev, tsStr)

	err = h.q.InsertAudit(context.Background(), db.InsertAuditParams{
		ID:             ev.ID,
		TraceID:        ev.TraceID,
		Actor:          ev.Actor,
		Timestamp:      tsStr,
		ServerID:       ev.ServerID,
		ServerName:     ev.ServerName,
		SshUser:        ev.SSHUser,
		ServerHost:     ev.ServerHost,
		Operator:       ev.Operator,
		UserID:         ev.UserID,
		Username:       ev.Username,
		Command:        ev.Command,
		Intent:         ev.Intent,
		DryRun:         int32(boolToInt(ev.DryRun)),
		Decision:       string(ev.Decision),
		L1VerdictJson:  nullJSON(ev.L1Verdict),
		L2VerdictJson:  nullJSON(ev.L2Verdict),
		ApprovalID:     nullString(ev.ApprovalID),
		ApprovalStatus: nullString(string(ev.ApprovalStatus)),
		ExitCode:       nullInt32(ev.ExitCode),
		DurationMs:     nullInt64FromPtr(ev.DurationMs),
		StdoutPreview:  nullString(ev.StdoutPreview),
		StderrPreview:  nullString(ev.StderrPreview),
		Error:          nullString(ev.Error),
		PrevHash:       ev.PrevHash,
		EventHash:      ev.EventHash,
	})
	if err != nil {
		return fmt.Errorf("audit insert: %w", err)
	}

	if h.auditJSONLPath != "" {
		h.appendJSONL(ev)
	}
	return nil
}

func (h *DB) appendJSONL(ev *domain.AuditEvent) {
	f, err := os.OpenFile(h.auditJSONLPath, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0o600) // #nosec G304 -- configured path
	if err != nil {
		return
	}
	defer f.Close()
	if b, err := json.Marshal(ev); err == nil {
		_, _ = f.Write(append(b, '\n'))
	}
}

// QueryAudits filters the audit log. The UserID dimension scopes results to
// one user (used for the audit.view_self permission level).
func (h *DB) QueryAudits(q *domain.AuditQuery) ([]*domain.AuditEvent, error) {
	params := auditQueryParams(q)
	rows, err := h.q.SelectAuditLogs(context.Background(), params)
	if err != nil {
		return nil, fmt.Errorf("query audits: %w", err)
	}
	out := make([]*domain.AuditEvent, 0, len(rows))
	for _, r := range rows {
		out = append(out, auditRowToEvent(r))
	}
	return out, nil
}

// auditQueryParams converts the domain query into sqlc nullable params.
func auditQueryParams(q *domain.AuditQuery) db.SelectAuditLogsParams {
	p := db.SelectAuditLogsParams{}
	p.ServerID = nullString(q.ServerID)
	p.SshUser = nullString(q.SSHUser)
	p.UserID = nullString(q.UserID)
	p.Username = nullString(q.Username)
	p.Decision = nullString(string(q.Decision))
	p.Actor = nullString(q.Actor)
	if q.From != nil {
		p.FromTs = nullString(formatRFC3339(*q.From))
	}
	if q.To != nil {
		p.ToTs = nullString(formatRFC3339(*q.To))
	}
	p.Q = nullString(q.Q)
	if q.OnlyFailed {
		p.OnlyFailed = nullInt32FromInt(1)
	}
	limit := int64(q.Limit)
	if limit <= 0 {
		limit = 200
	}
	if limit > 2000 {
		limit = 2000
	}
	p.Lim = nullInt64(limit)
	p.Offs = nullInt64(int64(q.Offset))
	return p
}

func auditRowToEvent(r db.AuditLog) *domain.AuditEvent {
	ev := &domain.AuditEvent{
		ID: r.ID, TraceID: r.TraceID, Actor: r.Actor, ServerID: r.ServerID, ServerName: r.ServerName,
		SSHUser: r.SshUser, ServerHost: r.ServerHost, Operator: r.Operator,
		UserID: r.UserID, Username: r.Username,
		Command: r.Command, Intent: r.Intent, DryRun: r.DryRun == 1,
		Decision: domain.PolicyDecision(r.Decision), PrevHash: r.PrevHash, EventHash: r.EventHash,
	}
	if t, ok := parseRFC3339(r.Timestamp); ok {
		ev.Timestamp = t
	}
	if r.L1VerdictJson.Valid && r.L1VerdictJson.String != "" {
		_ = json.Unmarshal([]byte(r.L1VerdictJson.String), &ev.L1Verdict)
	}
	if r.L2VerdictJson.Valid && r.L2VerdictJson.String != "" {
		_ = json.Unmarshal([]byte(r.L2VerdictJson.String), &ev.L2Verdict)
	}
	if r.ApprovalID.Valid {
		ev.ApprovalID = r.ApprovalID.String
	}
	if r.ApprovalStatus.Valid && r.ApprovalStatus.String != "" {
		ev.ApprovalStatus = domain.ApprovalDecision(r.ApprovalStatus.String)
	}
	if r.ExitCode.Valid {
		c := int(r.ExitCode.Int32)
		ev.ExitCode = &c
	}
	if r.DurationMs.Valid {
		d := r.DurationMs.Int64
		ev.DurationMs = &d
	}
	if r.StdoutPreview.Valid {
		ev.StdoutPreview = r.StdoutPreview.String
	}
	if r.StderrPreview.Valid {
		ev.StderrPreview = r.StderrPreview.String
	}
	if r.Error.Valid {
		ev.Error = r.Error.String
	}
	return ev
}

// VerifyAuditChain recomputes the full hash chain from genesis. The timestamp
// is hashed verbatim as stored, so chains written by earlier versions keep
// verifying unchanged.
func (h *DB) VerifyAuditChain() (*domain.AuditChainStatus, error) {
	rows, err := h.q.SelectAuditChain(context.Background())
	if err != nil {
		return nil, fmt.Errorf("verify chain: %w", err)
	}

	status := &domain.AuditChainStatus{Valid: true}
	var prevHash string
	for _, r := range rows {
		status.TotalEvents++
		expectedPrev := prevHash
		if expectedPrev == "" {
			expectedPrev = GenesisHash
		}
		ev := &domain.AuditEvent{
			ID: r.ID, TraceID: r.TraceID, Actor: r.Actor, Timestamp: mustParse(r.Timestamp),
			ServerID: r.ServerID, Command: r.Command, Intent: r.Intent,
			Decision: domain.PolicyDecision(r.Decision),
			SSHUser:  r.SshUser, ServerHost: r.ServerHost, Operator: r.Operator,
			UserID: r.UserID, Username: r.Username,
			PrevHash: r.PrevHash,
		}
		if r.ExitCode.Valid {
			c := int(r.ExitCode.Int32)
			ev.ExitCode = &c
		}
		if r.PrevHash != expectedPrev || computeEventHash(ev, r.Timestamp) != r.EventHash {
			status.Valid = false
			status.BrokenAtID = r.ID
			break
		}
		prevHash = r.EventHash
	}
	return status, nil
}

// exitCodeDebugFormat renders exit code exactly like the legacy format
// ("None" / "Some(0)"), keeping the hash payload stable.
func exitCodeDebugFormat(ev *domain.AuditEvent) string {
	if ev.ExitCode == nil {
		return "None"
	}
	return fmt.Sprintf("Some(%d)", *ev.ExitCode)
}

// computeEventHash builds the SHA-256 chain hash over the raw stored fields.
// The timestamp is hashed verbatim as stored; multi-user fields are appended
// only when present, so pre-existing chain records verify unchanged.
func computeEventHash(ev *domain.AuditEvent, tsStr string) string {
	payload := fmt.Sprintf("%s|%s|%s|%s|%s|%s|%s|%s|%s|%s",
		ev.PrevHash, ev.ID, ev.TraceID, ev.Actor, tsStr,
		ev.ServerID, ev.Command, ev.Intent, string(ev.Decision), exitCodeDebugFormat(ev))
	if ev.SSHUser != "" || ev.Operator != "" || ev.ServerHost != "" {
		payload += fmt.Sprintf("|%s|%s|%s", ev.SSHUser, ev.ServerHost, ev.Operator)
	}
	if ev.UserID != "" || ev.Username != "" {
		payload += fmt.Sprintf("|%s|%s", ev.UserID, ev.Username)
	}
	sum := sha256.Sum256([]byte(payload))
	return hex.EncodeToString(sum[:])
}

// nullJSON marshals a pointer payload into a nullable JSON string column.
func nullJSON(v any) sql.NullString {
	if v == nil {
		return sql.NullString{}
	}
	b, err := json.Marshal(v)
	if err != nil {
		return sql.NullString{}
	}
	return sql.NullString{String: string(b), Valid: true}
}

func nullInt64FromPtr(p *int64) sql.NullInt64 {
	if p == nil {
		return sql.NullInt64{}
	}
	return sql.NullInt64{Int64: *p, Valid: true}
}

func nullInt64(i int64) sql.NullInt64 {
	return sql.NullInt64{Int64: i, Valid: true}
}

func nullInt32(i *int) sql.NullInt32 {
	if i == nil {
		return sql.NullInt32{}
	}
	return sql.NullInt32{Int32: int32(*i), Valid: true}
}

func nullInt32FromInt(i int) sql.NullInt32 {
	return sql.NullInt32{Int32: int32(i), Valid: true}
}
