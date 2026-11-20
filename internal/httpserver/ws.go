package httpserver

import (
	"context"
	"strings"
	"time"

	"github.com/gofiber/contrib/websocket"
	"github.com/gofiber/fiber/v2"

	"github.com/yuantang/sshield/internal/apierr"
	"github.com/yuantang/sshield/internal/approvals"
	"github.com/yuantang/sshield/internal/domain"
	"github.com/yuantang/sshield/internal/sshpool"
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
	writeControl(conn, "ready", "")
	s.Audit(&domain.AuditEvent{
		ID: newAuditID(), Actor: "terminal", Timestamp: time.Now().UTC(),
		ServerID: asset.ID, ServerName: asset.Name,
		SSHUser: asset.Username, ServerHost: hostPort(asset),
		UserID: identity.UserID, Username: identity.Username,
		Command: "terminal.open", Intent: "interactive PTY session",
		Decision: domain.DecisionAllow,
	})

	// PTY output → websocket (binary frames).
	go func() {
		for {
			select {
			case out, ok := <-session.Output():
				if !ok {
					return
				}
				if out.Closed {
					_ = conn.WriteMessage(websocket.TextMessage, []byte(`{"type":"exit"}`))
					return
				}
				if conn.WriteMessage(websocket.BinaryMessage, out.Data) != nil {
					return
				}
			case <-session.Done():
				return
			}
		}
	}()

	// Command tracking for the audit chain (submitted lines, best-effort).
	tracker := sshpool.NewCommandTracker()

	// Websocket input → PTY.
	for {
		mt, payload, err := conn.ReadMessage()
		if err != nil {
			session.Close()
			return
		}
		switch mt {
		case websocket.TextMessage:
			in, err := sshpool.MarshalInput(payload)
			if err != nil {
				continue
			}
			if in.Resize {
				session.Input() <- sshpool.TerminalInput{Resize: true, Cols: in.Cols, Rows: in.Rows}
				continue
			}
			for _, cmd := range tracker.Feed(in.Data) {
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
			session.Input() <- sshpool.TerminalInput{Data: in.Data}
		case websocket.BinaryMessage:
			for _, cmd := range tracker.Feed(payload) {
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
			session.Input() <- sshpool.TerminalInput{Data: payload}
		}
	}
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
