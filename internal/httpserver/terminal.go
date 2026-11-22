package httpserver

import (
	"context"
	"strings"
	"sync"
	"time"

	"github.com/yuantang/sshield/internal/domain"
	"github.com/yuantang/sshield/internal/metrics"
	"github.com/yuantang/sshield/internal/notify"
	"github.com/yuantang/sshield/internal/storage"
)

// approvalWaitTimeout bounds how long an interactive terminal command may
// block on a human approval decision (same budget as MCP executions).
const approvalWaitTimeout = 120 * time.Second

// terminalVerdict is the outcome of running one interactive line through the
// layered pipeline (L1 AST rules → L2 semantic review → human approval).
type terminalVerdict struct {
	Decision       domain.PolicyDecision
	L1             *domain.CommandVerdict
	L2             *domain.SemanticReviewVerdict
	ApprovalID     string
	ApprovalStatus domain.ApprovalDecision
	Reason         string
}

// evaluateTerminalLine mirrors the MCP exec pipeline for interactive
// terminal lines: L1 is always evaluated (fail-closed), L2 when a reviewer is
// configured, and ask_approval blocks on the approvals broker.
func (s *Server) evaluateTerminalLine(ctx context.Context, id *domain.Identity, asset *domain.ServerAsset, cmd string) *terminalVerdict {
	verdict := s.engine.Evaluate(cmd, s.CachedRules())
	v := &terminalVerdict{Decision: verdict.Decision, L1: verdict}
	if verdict.Decision == domain.DecisionDeny {
		v.Reason = verdict.Reason
		return v
	}

	var l2 *domain.SemanticReviewVerdict
	if reviewer := s.L2Reviewer(); reviewer != nil {
		result, err := reviewer.Review(ctx, &domain.SemanticReviewRequest{
			ServerName: asset.Name, ServerTags: asset.Tags,
			Command: cmd, Intent: "interactive terminal command", L1Verdict: *verdict,
		})
		if err != nil {
			// L2 fails closed.
			return &terminalVerdict{Decision: domain.DecisionAskApproval, L1: verdict, Reason: err.Error()}
		}
		l2 = result
	}
	v.L2 = l2
	if l2 != nil {
		v.Decision = domain.MoreRestrictive(v.Decision, l2.Decision)
	}

	if v.Decision != domain.DecisionAskApproval {
		return v
	}

	req := domain.ApprovalRequest{
		ID:          storage.GenerateID("appr"),
		TraceID:     "term-" + asset.ID,
		CreatedAt:   time.Now().UTC(),
		ServerID:    asset.ID,
		ServerName:  asset.Name,
		Host:        asset.Host,
		Username:    asset.Username,
		Command:     cmd,
		Intent:      "interactive terminal command",
		Verdict:     *verdict,
		L2Verdict:   l2,
		TimeoutSecs: int64(approvalWaitTimeout.Seconds()),
		UserID:      id.UserID,
		RequestedBy: id.Username,
	}
	s.notify.Send("approval.requested",
		"Approval requested for interactive terminal command on "+asset.Name,
		map[string]any{"approval_id": req.ID, "server": asset.Name, "user": id.Username, "command": notify.TruncateCommand(cmd)})
	decision := s.broker.Request(req, approvalWaitTimeout)
	v.ApprovalID = req.ID
	v.ApprovalStatus = decision
	if decision != domain.ApprovalApproved {
		v.Reason = "approval " + string(decision) + " for command on " + asset.Name
		return v
	}
	return v
}

// Terminal event directions.
const (
	directionOutput = "output"
	directionInput  = "input"
	directionNotice = "notice"
)

// --- session recorder ---

// sessionRecorder persists one terminal session's event stream. Events are
// queued and flushed by a background goroutine; the byte cap truncates the
// output stream once the per-session budget is exhausted.
type sessionRecorder struct {
	db        *storage.DB
	sessionID string
	openedAt  time.Time
	maxBytes  int64

	mu       sync.Mutex
	pending  []domain.TerminalEvent
	byteSize int64
	events   int64
	truncated bool
	done     chan struct{}
	closed   bool
}

func newSessionRecorder(db *storage.DB, sess *domain.TerminalSession, maxBytes int64) *sessionRecorder {
	r := &sessionRecorder{
		db:        db,
		sessionID: sess.ID,
		openedAt:  sess.OpenedAt,
		maxBytes:  maxBytes,
		done:      make(chan struct{}),
	}
	if r.maxBytes <= 0 {
		r.maxBytes = 10 * 1024 * 1024
	}
	go r.flushLoop()
	return r
}

// record queues one event. Output bytes beyond the cap are dropped and the
// recording is flagged truncated.
func (r *sessionRecorder) record(direction, data string) {
	if r == nil {
		return
	}
	r.mu.Lock()
	if r.closed {
		r.mu.Unlock()
		return
	}
	if direction == "output" {
		if r.byteSize+int64(len(data)) > r.maxBytes {
			room := r.maxBytes - r.byteSize
			if room > 0 {
				data = data[:room]
				r.pending = append(r.pending, domain.TerminalEvent{TsMs: r.elapsed(), Direction: direction, Data: data})
				r.byteSize += room
				r.events++
			}
			r.truncated = true
			r.mu.Unlock()
			return
		}
		r.byteSize += int64(len(data))
	}
	r.pending = append(r.pending, domain.TerminalEvent{TsMs: r.elapsed(), Direction: direction, Data: data})
	r.events++
	r.mu.Unlock()
}

func (r *sessionRecorder) elapsed() int64 {
	return time.Since(r.openedAt).Milliseconds()
}

// flushLoop batches queued events into the database.
func (r *sessionRecorder) flushLoop() {
	ticker := time.NewTicker(300 * time.Millisecond)
	defer ticker.Stop()
	for {
		select {
		case <-r.done:
			r.flush()
			return
		case <-ticker.C:
			r.flush()
		}
	}
}

func (r *sessionRecorder) flush() {
	r.mu.Lock()
	if len(r.pending) == 0 {
		r.mu.Unlock()
		return
	}
	batch := r.pending
	r.pending = nil
	r.mu.Unlock()

	for _, ev := range batch {
		_ = r.db.AppendTerminalEvent(r.sessionID, ev.TsMs, ev.Direction, ev.Data)
	}
}

// close flushes pending events and finalizes the session row. Best-effort:
// recording failures never break the terminal itself.
func (r *sessionRecorder) close(reason string) {
	if r == nil {
		return
	}
	r.mu.Lock()
	if r.closed {
		r.mu.Unlock()
		return
	}
	r.pending = append(r.pending, domain.TerminalEvent{
		TsMs: r.elapsed(), Direction: directionNotice, Data: "[recording closed: " + reason + "]",
	})
	r.closed = true
	truncated := r.truncated
	byteSize := r.byteSize
	events := r.events
	r.mu.Unlock()

	close(r.done)
	_ = r.db.CloseTerminalSession(r.sessionID, reason, time.Now().UTC(), byteSize, events, truncated)
}

// --- interactive policy gate (enforce mode) ---

// terminalGate intercepts Enter in the PTY input stream: typed characters are
// forwarded as usual (remote echo, tab completion and remote line editing all
// keep working), but an Enter carrying a pending command line first runs the
// policy pipeline. On deny the remote line buffer is cleared with Ctrl+U
// (0x15) so the command never executes, and a notice is shown. Multi-line
// pastes are evaluated line by line, in order.
type terminalGate struct {
	line    strings.Builder
	enforce bool
}

func newTerminalGate(enforce bool) *terminalGate {
	return &terminalGate{enforce: enforce}
}

func (g *terminalGate) pending() string {
	return strings.TrimSpace(g.line.String())
}

// process runs the input frame through the gate. evaluate is called for each
// Enter-terminated non-empty line; returning false denies the line (the
// remote line buffer is killed with Ctrl+U and the Enter byte is withheld).
func (g *terminalGate) process(data []byte, evaluate func(line string) bool) []byte {
	out := make([]byte, 0, len(data))
	for i := 0; i < len(data); i++ {
		b := data[i]
		switch b {
		case 0x03, 0x15: // Ctrl+C / Ctrl+U clear the pending line
			g.line.Reset()
			out = append(out, b)
		case '\r', '\n':
			pending := g.pending()
			g.line.Reset()
			if pending != "" && g.enforce && !evaluate(pending) {
				// Deny: kill the remote line buffer, withhold Enter.
				out = append(out, 0x15)
				continue
			}
			out = append(out, b)
		default:
			if b >= 0x20 && b < 0x7f || b >= 0xa0 {
				g.line.WriteByte(b)
			}
			out = append(out, b)
		}
	}
	return out
}

// --- shared audit/metric helpers ---

func (s *Server) auditTerminalLine(id *domain.Identity, asset *domain.ServerAsset, traceID string, cmd string, v *terminalVerdict, errMsg string) {
	decision := domain.DecisionAllow
	if v != nil {
		decision = v.Decision
	}
	ev := &domain.AuditEvent{
		ID: storage.GenerateID("aud"), TraceID: traceID, Actor: "terminal",
		Timestamp: time.Now().UTC(),
		ServerID:  asset.ID, ServerName: asset.Name,
		SSHUser: asset.Username, ServerHost: hostPort(asset),
		UserID: id.UserID, Username: id.Username,
		Command: cmd, Intent: "interactive terminal command",
		Decision: decision,
		Error:    errMsg,
	}
	if v != nil {
		ev.L1Verdict = v.L1
		ev.L2Verdict = v.L2
		ev.ApprovalID = v.ApprovalID
		ev.ApprovalStatus = v.ApprovalStatus
	}
	s.Audit(ev)
}

func recordDenialMetrics(channel string) {
	metrics.PolicyDenialsTotal.WithLabelValues(channel).Inc()
}
