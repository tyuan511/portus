package storage

import (
	"crypto/sha256"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"strings"

	"github.com/yuantang/sshield/internal/domain"
)

// GenesisHash is the prev_hash of the first audit record.
const GenesisHash = "GENESIS"

// auditRow mirrors the raw audit_logs row; string timestamps keep byte-level
// compatibility with chains written by the legacy (Rust) implementation.
type auditRow struct {
	Seq          int64
	ID           string
	TraceID      string
	Actor        string
	Timestamp    string
	ServerID     string
	ServerName   string
	SSHUser      string
	ServerHost   string
	Operator     string
	UserID       string
	Username     string
	Command      string
	Intent       string
	DryRun       int
	Decision     string
	L1JSON       sql.NullString
	L2JSON       sql.NullString
	ApprovalID   sql.NullString
	ApprovalStat sql.NullString
	ExitCode     sql.NullInt64
	DurationMs   sql.NullInt64
	StdoutPrev   sql.NullString
	StderrPrev   sql.NullString
	Error        sql.NullString
	PrevHash     string
	EventHash    string
}

const auditCols = `seq, id, trace_id, actor, timestamp, server_id, server_name, ssh_user,
	server_host, operator, user_id, username, command, intent, dry_run, decision,
	l1_verdict_json, l2_verdict_json, approval_id, approval_status, exit_code,
	duration_ms, stdout_preview, stderr_preview, error, prev_hash, event_hash`

func scanAuditRow(row interface{ Scan(...any) error }) (*auditRow, error) {
	var r auditRow
	err := row.Scan(&r.Seq, &r.ID, &r.TraceID, &r.Actor, &r.Timestamp, &r.ServerID, &r.ServerName,
		&r.SSHUser, &r.ServerHost, &r.Operator, &r.UserID, &r.Username, &r.Command, &r.Intent,
		&r.DryRun, &r.Decision, &r.L1JSON, &r.L2JSON, &r.ApprovalID, &r.ApprovalStat,
		&r.ExitCode, &r.DurationMs, &r.StdoutPrev, &r.StderrPrev, &r.Error, &r.PrevHash, &r.EventHash)
	return &r, err
}

func (r *auditRow) toEvent() (*domain.AuditEvent, error) {
	ev := &domain.AuditEvent{
		ID: r.ID, TraceID: r.TraceID, Actor: r.Actor, ServerID: r.ServerID, ServerName: r.ServerName,
		SSHUser: r.SSHUser, ServerHost: r.ServerHost, Operator: r.Operator,
		UserID: r.UserID, Username: r.Username,
		Command: r.Command, Intent: r.Intent, DryRun: intToBool(r.DryRun),
		Decision: domain.PolicyDecision(r.Decision), PrevHash: r.PrevHash, EventHash: r.EventHash,
	}
	if t, ok := parseRFC3339(r.Timestamp); ok {
		ev.Timestamp = t
	}
	if r.L1JSON.Valid && r.L1JSON.String != "" {
		_ = json.Unmarshal([]byte(r.L1JSON.String), &ev.L1Verdict)
	}
	if r.L2JSON.Valid && r.L2JSON.String != "" {
		_ = json.Unmarshal([]byte(r.L2JSON.String), &ev.L2Verdict)
	}
	if r.ApprovalID.Valid {
		ev.ApprovalID = r.ApprovalID.String
	}
	if r.ApprovalStat.Valid && r.ApprovalStat.String != "" {
		decision := domain.ApprovalDecision(r.ApprovalStat.String)
		ev.ApprovalStatus = decision
	}
	if r.ExitCode.Valid {
		c := int(r.ExitCode.Int64)
		ev.ExitCode = &c
	}
	if r.DurationMs.Valid {
		d := r.DurationMs.Int64
		ev.DurationMs = &d
	}
	if r.StdoutPrev.Valid {
		ev.StdoutPreview = r.StdoutPrev.String
	}
	if r.StderrPrev.Valid {
		ev.StderrPreview = r.StderrPrev.String
	}
	if r.Error.Valid {
		ev.Error = r.Error.String
	}
	return ev, nil
}

// exitCodeDebugFormat renders exit code exactly like Rust's {:?} on Option<i32>
// ("None" / "Some(0)"), keeping hash chains byte-compatible with legacy records.
func exitCodeDebugFormat(ev *domain.AuditEvent) string {
	if ev.ExitCode == nil {
		return "None"
	}
	return fmt.Sprintf("Some(%d)", *ev.ExitCode)
}

// computeEventHash builds the SHA-256 chain hash over the raw stored fields.
// The timestamp is hashed verbatim as stored (the legacy implementation hashed
// exactly the string it persisted), so pre-existing chains verify unchanged.
// New multi-user fields are appended only when present.
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

// AppendAudit inserts an event into the hash chain (and optional JSONL mirror).
func (db *DB) AppendAudit(ev *domain.AuditEvent) error {
	if ev.TraceID == "" {
		ev.TraceID = ev.ID
	}
	if ev.Timestamp.IsZero() {
		ev.Timestamp = nowUTC()
	}
	tsStr := formatRFC3339(ev.Timestamp)

	var prevHash string
	err := db.sql.QueryRow(`SELECT event_hash FROM audit_logs ORDER BY seq DESC LIMIT 1`).Scan(&prevHash)
	switch {
	case errors.Is(err, sql.ErrNoRows):
		prevHash = GenesisHash
	case err != nil:
		return fmt.Errorf("audit tail: %w", err)
	}

	ev.PrevHash = prevHash
	ev.EventHash = computeEventHash(ev, tsStr)

	l1 := nullJSON(ev.L1Verdict)
	l2 := nullJSON(ev.L2Verdict)
	_, err = db.sql.Exec(`INSERT INTO audit_logs (`+auditCols+`)
		VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)`,
		nil, ev.ID, ev.TraceID, ev.Actor, tsStr,
		ev.ServerID, ev.ServerName, ev.SSHUser, ev.ServerHost, ev.Operator,
		ev.UserID, ev.Username, ev.Command, ev.Intent, boolToInt(ev.DryRun),
		string(ev.Decision), l1, l2,
		nullString(ev.ApprovalID), nullString(string(ev.ApprovalStatus)),
		nullInt64Ptr(ev.ExitCode), nullInt64Ptr(ev.DurationMs),
		nullString(ev.StdoutPreview), nullString(ev.StderrPreview), nullString(ev.Error),
		ev.PrevHash, ev.EventHash)
	if err != nil {
		return fmt.Errorf("audit insert: %w", err)
	}

	if db.auditJSONLPath != "" {
		db.appendJSONL(ev)
	}
	return nil
}

func (db *DB) appendJSONL(ev *domain.AuditEvent) {
	f, err := os.OpenFile(db.auditJSONLPath, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0o600) // #nosec G304 -- configured path
	if err != nil {
		return
	}
	defer f.Close()
	if b, err := json.Marshal(ev); err == nil {
		_, _ = f.Write(append(b, '\n'))
	}
}

// QueryAudits filters the audit log. userIDFilter scopes results to one user
// (used for the audit.view_self permission level).
func (db *DB) QueryAudits(q *domain.AuditQuery) ([]*domain.AuditEvent, error) {
	var sb strings.Builder
	var args []any
	sb.WriteString(`SELECT ` + auditCols + ` FROM audit_logs WHERE 1=1`)

	if q.ServerID != "" {
		sb.WriteString(" AND server_id = ?")
		args = append(args, q.ServerID)
	}
	if q.SSHUser != "" {
		sb.WriteString(" AND ssh_user = ?")
		args = append(args, q.SSHUser)
	}
	if q.UserID != "" {
		sb.WriteString(" AND user_id = ?")
		args = append(args, q.UserID)
	} else if q.Username != "" {
		sb.WriteString(" AND username = ?")
		args = append(args, q.Username)
	}
	if q.Actor != "" {
		cond, arg := actorFilter(q.Actor)
		sb.WriteString(" AND " + cond)
		if arg != nil {
			args = append(args, arg)
		}
	}
	if q.Decision != "" {
		sb.WriteString(" AND decision = ?")
		args = append(args, string(q.Decision))
	}
	if q.OnlyFailed {
		sb.WriteString(" AND ((exit_code IS NOT NULL AND exit_code != 0) OR (error IS NOT NULL AND error != ''))")
	}
	if q.From != nil {
		sb.WriteString(" AND timestamp >= ?")
		args = append(args, formatRFC3339(*q.From))
	}
	if q.To != nil {
		sb.WriteString(" AND timestamp <= ?")
		args = append(args, formatRFC3339(*q.To))
	}
	if q.Q != "" {
		like := "%" + q.Q + "%"
		sb.WriteString(` AND (command LIKE ? OR intent LIKE ? OR server_name LIKE ?
			OR ssh_user LIKE ? OR server_host LIKE ? OR operator LIKE ? OR trace_id LIKE ?
			OR username LIKE ?)`)
		for i := 0; i < 8; i++ {
			args = append(args, like)
		}
	}

	limit := q.Limit
	if limit <= 0 {
		limit = 200
	}
	if limit > 2000 {
		limit = 2000
	}
	sb.WriteString(" ORDER BY seq DESC LIMIT ? OFFSET ?")
	args = append(args, limit, q.Offset)

	rows, err := db.sql.Query(sb.String(), args...)
	if err != nil {
		return nil, fmt.Errorf("query audits: %w", err)
	}
	defer rows.Close()

	var out []*domain.AuditEvent
	for rows.Next() {
		r, err := scanAuditRow(rows)
		if err != nil {
			return nil, err
		}
		ev, err := r.toEvent()
		if err != nil {
			return nil, err
		}
		out = append(out, ev)
	}
	return out, rows.Err()
}

// VerifyAuditChain recomputes the full hash chain from genesis.
func (db *DB) VerifyAuditChain() (*domain.AuditChainStatus, error) {
	rows, err := db.sql.Query(`SELECT ` + auditCols + ` FROM audit_logs ORDER BY seq ASC`)
	if err != nil {
		return nil, fmt.Errorf("verify chain: %w", err)
	}
	defer rows.Close()

	status := &domain.AuditChainStatus{Valid: true}
	var prevHash string
	for rows.Next() {
		r, err := scanAuditRow(rows)
		if err != nil {
			return nil, err
		}
		status.TotalEvents++
		expectedPrev := prevHash
		if expectedPrev == "" {
			expectedPrev = GenesisHash
		}
		ev := &domain.AuditEvent{
			ID: r.ID, TraceID: r.TraceID, Actor: r.Actor, Timestamp: mustParse(r.Timestamp),
			ServerID: r.ServerID, Command: r.Command, Intent: r.Intent,
			Decision: domain.PolicyDecision(r.Decision),
			SSHUser: r.SSHUser, ServerHost: r.ServerHost, Operator: r.Operator,
			UserID: r.UserID, Username: r.Username,
			PrevHash: r.PrevHash,
			ExitCode: nullIntToPtr(r.ExitCode),
		}
		if r.PrevHash != expectedPrev || computeEventHash(ev, r.Timestamp) != r.EventHash {
			status.Valid = false
			status.BrokenAtID = r.ID
			break
		}
		prevHash = r.EventHash
	}
	return status, rows.Err()
}

// nullJSON marshals a pointer payload or returns nil SQL value.
func nullJSON(v any) any {
	if v == nil {
		return nil
	}
	b, err := json.Marshal(v)
	if err != nil {
		return nil
	}
	return string(b)
}

func nullInt64Ptr[T int | int64](p *T) any {
	if p == nil {
		return nil
	}
	return *p
}

func nullIntToPtr(n sql.NullInt64) *int {
	if !n.Valid {
		return nil
	}
	v := int(n.Int64)
	return &v
}
