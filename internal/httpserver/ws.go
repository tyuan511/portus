package httpserver

import (
	"context"
	"net"
	"strings"
	"sync"
	"time"

	"github.com/gofiber/contrib/websocket"
	"github.com/gofiber/fiber/v2"

	"github.com/yuantang/sshield/internal/apierr"
	"github.com/yuantang/sshield/internal/approvals"
	"github.com/yuantang/sshield/internal/domain"
	"github.com/yuantang/sshield/internal/metrics"
	"github.com/yuantang/sshield/internal/notify"
	"github.com/yuantang/sshield/internal/sshpool"
	"github.com/yuantang/sshield/internal/storage"
)

// registerWebSockets mounts the approval event stream and the interactive
// terminal bridge. Both authenticate via the session cookie at upgrade time.
func (s *Server) registerWebSockets(app *fiber.App) {
	app.Get("/ws", s.requireAuth(), websocketUpgrade(), websocket.New(s.handleApprovalsWS))
	app.Get("/ws/terminal/:id", s.requireAuth(), websocketUpgrade(), websocket.New(s.handleTerminalWS))
}

func websocketUpgrade() fiber.Handler {
	return func(c *fiber.Ctx) error {
		if websocket.IsWebSocketUpgrade(c) {
			c.Locals("allowed", true)
			return c.Next()
		}
		return apierr.BadRequest("websocket upgrade required")
	}
}

// handleApprovalsWS streams approval events; only sessions with
// approvals.decide receive request payloads.
func (s *Server) handleApprovalsWS(conn *websocket.Conn) {
	identity := identityFromWS(conn)
	if identity == nil {
		_ = conn.Close()
		return
	}
	canDecide := identity.HasPermission(domain.PermApprovalsDecide)

	events, cancel := s.broker.Subscribe(func(req *domain.ApprovalRequest) bool { return canDecide })
	defer cancel()

	// Snapshot of currently pending approvals for the fresh connection.
	if canDecide {
		for _, req := range s.broker.ListPending() {
			snapshot := req
			if writeJSON(conn, approvals.Event{Type: "approval_requested", Payload: &snapshot}) != nil {
				return
			}
		}
	}

	done := make(chan struct{})
	go func() {
		defer close(done)
		for {
			if _, _, err := conn.ReadMessage(); err != nil {
				return
			}
		}
	}()

	for {
		select {
		case ev, ok := <-events:
			if !ok {
				return
			}
			if writeJSON(conn, ev) != nil {
				return
			}
		case <-done:
			return
		case <-time.After(25 * time.Second):
			// Ping to keep intermediaries from reaping the idle connection.
			if writeJSON(conn, approvals.Event{Type: "ping"}) != nil {
				return
			}
		}
	}
}

// handleTerminalWS bridges the browser xterm.js client to a PTY after
// verifying host.connect permission AND a connect grant on this host.
// Depending on configuration the session is recorded (terminal.record) and
// every submitted line is routed through the policy pipeline
// (terminal.enforce); denials kill the remote line with Ctrl+U before it can
// execute.
func (s *Server) handleTerminalWS(conn *websocket.Conn) {
	identity := identityFromWS(conn)
	if identity == nil {
		_ = conn.Close()
		return
	}
	serverID := conn.Params("id")

	asset, err := s.db.GetServer(serverID)
	if err != nil {
		writeControl(conn, "error", "server not found")
		_ = conn.Close()
		return
	}
	if !asset.Enabled {
		writeControl(conn, "error", "server is disabled")
		_ = conn.Close()
		return
	}
	if err := s.rbac.CheckServerAccess(identity, asset, false); err != nil {
		writeControl(conn, "error", err.Error())
		_ = conn.Close()
		return
	}

	cols := uint32(120)
	rows := uint32(36)
	if v := conn.Query("cols"); v != "" {
		if n, ok := parseUint(v); ok {
			cols = n
		}
	}
	if v := conn.Query("rows"); v != "" {
		if n, ok := parseUint(v); ok {
			rows = n
		}
	}

	all, err := s.db.ListServers()
	if err != nil {
		writeControl(conn, "error", err.Error())
		return
	}

	traceID := "term-" + serverID
	session, err := s.pool.OpenTerminal(context.Background(), asset, assetMap(all), cols, rows)
	if err != nil {
		writeControl(conn, "error", err.Error())
		_ = conn.Close()
		return
	}

	enforce := s.cfg.Terminal.Enforce
	policyMode := domain.TerminalModeAudit
	if enforce {
		policyMode = domain.TerminalModeEnforce
	}
	sess := &domain.TerminalSession{
		ID:         storage.GenerateID("tser"),
		UserID:     identity.UserID,
		Username:   identity.Username,
		ServerID:   asset.ID,
		ServerName: asset.Name,
		SSHUser:    asset.Username,
		ServerHost: hostPort(asset),
		ClientIP:   clientIPOf(conn),
		PolicyMode: policyMode,
		OpenedAt:   time.Now().UTC(),
	}
	var recorder *sessionRecorder
	if s.cfg.Terminal.Record {
		if err := s.db.CreateTerminalSession(sess); err == nil {
			recorder = newSessionRecorder(s.db, sess, s.cfg.Terminal.RecordMaxBytes)
		}
	}
	metrics.ActiveTerminals.Inc()

	// Serialized websocket writes: the output pump and the input loop both
	// write frames (data / notices / exit).
	var writeMu sync.Mutex
	writeBinary := func(data []byte) {
		writeMu.Lock()
		defer writeMu.Unlock()
		if conn.WriteMessage(websocket.BinaryMessage, data) != nil {
			// Connection is going away; nothing to recover.
			_ = conn.Close()
		}
	}
	writeNotice := func(msg string) {
		writeMu.Lock()
		defer writeMu.Unlock()
		_ = conn.WriteMessage(websocket.TextMessage, []byte(`{"type":"notice","message":`+jsonString(msg)+`}`))
	}

	writeMu.Lock()
	_ = conn.WriteMessage(websocket.TextMessage, []byte(`{"type":"ready","policy":`+jsonString(policyMode)+`}`))
	writeMu.Unlock()
	s.Audit(&domain.AuditEvent{
		ID: newAuditID(), Actor: "terminal", Timestamp: time.Now().UTC(),
		ServerID: asset.ID, ServerName: asset.Name,
		SSHUser: asset.Username, ServerHost: hostPort(asset),
		UserID: identity.UserID, Username: identity.Username,
		Command: "terminal.open", Intent: "interactive PTY session (" + policyMode + ")",
		Decision: domain.DecisionAllow,
	})
	recorder.record(directionNotice, "[session opened: "+policyMode+" mode, user "+identity.Username+"]")

	closed := make(chan struct{})
	var closeOnce sync.Once
	closeSession := func(reason string) {
		closeOnce.Do(func() {
			session.Close()
			metrics.ActiveTerminals.Dec()
			recorder.close(reason)
			s.Audit(&domain.AuditEvent{
				ID: newAuditID(), Actor: "terminal", Timestamp: time.Now().UTC(),
				TraceID:  traceID,
				ServerID: asset.ID, ServerName: asset.Name,
				SSHUser: asset.Username, ServerHost: hostPort(asset),
				UserID: identity.UserID, Username: identity.Username,
				Command: "terminal.close", Intent: "interactive PTY session ended",
				Decision: domain.DecisionAllow, Note: reason,
			})
			close(closed)
		})
	}

	// PTY output → websocket (binary frames), recorded when enabled.
	go func() {
		for {
			select {
			case out, ok := <-session.Output():
				if !ok {
					closeSession("session ended")
					return
				}
				if out.Closed {
					writeMu.Lock()
					_ = conn.WriteMessage(websocket.TextMessage, []byte(`{"type":"exit"}`))
					writeMu.Unlock()
					closeSession("remote closed")
					return
				}
				recorder.record(directionOutput, string(out.Data))
				writeBinary(out.Data)
			case <-session.Done():
				closeSession("session ended")
				return
			case <-closed:
				return
			}
		}
	}()

	// Policy evaluation for one submitted line (enforce mode). Returns true
	// when the line may proceed to the remote shell.
	evaluateLine := func(cmd string) bool {
		verdict := s.evaluateTerminalLine(context.Background(), identity, asset, cmd)
		recorder.record(directionInput, cmd)
		switch verdict.Decision {
		case domain.DecisionDeny:
			recorder.record(directionNotice, "[blocked by policy: "+verdict.Reason+"] "+cmd)
			writeNotice("sshield: command blocked by policy — " + verdict.Reason)
			s.auditTerminalLine(identity, asset, traceID, cmd, verdict, verdict.Reason)
			recordDenialMetrics("terminal")
			s.notify.Send("policy.terminal_denied",
				"Terminal command blocked by policy on "+asset.Name,
				map[string]any{"server": asset.Name, "user": identity.Username, "command": notify.TruncateCommand(cmd), "reason": verdict.Reason})
			return false
		case domain.DecisionAskApproval:
			if verdict.ApprovalStatus != domain.ApprovalApproved {
				recorder.record(directionNotice, "[approval "+string(verdict.ApprovalStatus)+"] "+cmd)
				writeNotice("sshield: approval " + string(verdict.ApprovalStatus) + " — command not executed")
				s.auditTerminalLine(identity, asset, traceID, cmd, verdict, verdict.Reason)
				recordDenialMetrics("terminal")
				metrics.ApprovalsDecisionsTotal.WithLabelValues(string(verdict.ApprovalStatus)).Inc()
				s.notify.Send("approval.decided",
					"Approval "+string(verdict.ApprovalStatus)+" for terminal command on "+asset.Name,
					map[string]any{"approval_id": verdict.ApprovalID, "server": asset.Name, "user": identity.Username, "command": notify.TruncateCommand(cmd)})
				return false
			}
			metrics.ApprovalsDecisionsTotal.WithLabelValues("approved").Inc()
			s.notify.Send("approval.decided",
				"Approval granted for terminal command on "+asset.Name,
				map[string]any{"approval_id": verdict.ApprovalID, "server": asset.Name, "user": identity.Username})
			s.auditTerminalLine(identity, asset, traceID, cmd, verdict, "")
			return true
		default:
			metrics.ExecutionsTotal.WithLabelValues("terminal", string(verdict.Decision)).Inc()
			s.auditTerminalLine(identity, asset, traceID, cmd, verdict, "")
			return true
		}
	}

	// The gate persists across frames so partially typed lines keep
	// accumulating until Enter.
	gate := newTerminalGate(enforce)

	// Command tracking for the audit chain in audit mode (submitted lines).
	tracker := sshpool.NewCommandTracker()

	// Websocket input → PTY (policy-gated in enforce mode).
	for {
		mt, payload, err := conn.ReadMessage()
		if err != nil {
			closeSession("client disconnected")
			return
		}
		data := payload
		if mt == websocket.TextMessage {
			in, err := sshpool.MarshalInput(payload)
			if err != nil {
				continue
			}
			if in.Resize {
				session.Input() <- sshpool.TerminalInput{Resize: true, Cols: in.Cols, Rows: in.Rows}
				continue
			}
			data = in.Data
		}

		if enforce {
			forwarded := gate.process(data, evaluateLine)
			if len(forwarded) > 0 {
				session.Input() <- sshpool.TerminalInput{Data: forwarded}
			}
			continue
		}

		// Audit mode: forward everything, track submitted lines for audit.
		for _, cmd := range tracker.Feed(data) {
			recorder.record(directionInput, cmd)
			metrics.ExecutionsTotal.WithLabelValues("terminal", string(domain.DecisionAllow)).Inc()
			s.Audit(&domain.AuditEvent{
				ID: newAuditID(), Actor: "terminal", Timestamp: time.Now().UTC(),
				TraceID:  traceID,
				ServerID: asset.ID, ServerName: asset.Name,
				SSHUser: asset.Username, ServerHost: hostPort(asset),
				UserID: identity.UserID, Username: identity.Username,
				Command: cmd, Intent: "manual interactive terminal",
				Decision: domain.DecisionAllow,
			})
		}
		session.Input() <- sshpool.TerminalInput{Data: data}
	}
}

func clientIPOf(conn *websocket.Conn) string {
	if conn.RemoteAddr() == nil {
		return ""
	}
	if host, _, err := net.SplitHostPort(conn.RemoteAddr().String()); err == nil {
		return host
	}
	return conn.RemoteAddr().String()
}

func identityFromWS(conn *websocket.Conn) *domain.Identity {
	id, _ := conn.Locals("identity").(*domain.Identity)
	return id
}

func writeJSON(conn *websocket.Conn, v any) error {
	return conn.WriteJSON(v)
}

func writeControl(conn *websocket.Conn, typ, message string) {
	payload := `{"type":"` + typ + `"}`
	if message != "" && typ == "error" {
		payload = `{"type":"error","message":` + jsonString(message) + `}`
	}
	_ = conn.WriteMessage(websocket.TextMessage, []byte(payload))
}

func jsonString(s string) string {
	var sb strings.Builder
	sb.WriteByte('"')
	for _, r := range s {
		switch r {
		case '"':
			sb.WriteString(`\"`)
		case '\\':
			sb.WriteString(`\\`)
		case '\n':
			sb.WriteString(`\n`)
		default:
			sb.WriteRune(r)
		}
	}
	sb.WriteByte('"')
	return sb.String()
}

func hostPort(a *domain.ServerAsset) string {
	return a.Host + ":" + itoa(a.Port)
}

func parseUint(s string) (uint32, bool) {
	n := uint64(0)
	for _, r := range s {
		if r < '0' || r > '9' {
			return 0, false
		}
		n = n*10 + uint64(r-'0')
		if n > 1<<32-1 {
			return 0, false
		}
	}
	if n == 0 {
		return 0, false
	}
	return uint32(n), true
}

func itoa(n int) string {
	if n == 0 {
		return "0"
	}
	var buf [20]byte
	i := len(buf)
	for n > 0 {
		i--
		buf[i] = byte('0' + n%10)
		n /= 10
	}
	return string(buf[i:])
}
