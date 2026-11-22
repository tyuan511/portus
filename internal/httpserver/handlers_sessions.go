package httpserver

import (
	"strconv"

	"github.com/gofiber/fiber/v2"

	"github.com/yuantang/sshield/internal/apierr"
	"github.com/yuantang/sshield/internal/domain"
	"github.com/yuantang/sshield/internal/httpserver/resp"
)

// registerMCPSessionRoutes mounts the MCP session replay API. Sessions are
// groupings of audit events by agent-supplied session ids; scoping mirrors
// the audit views (members see own sessions, admins see all).
func (s *Server) registerMCPSessionRoutes(g fiber.Router) {
	g.Get("/mcp-sessions", s.handleListMCPSessions)
	g.Get("/mcp-sessions/:id/events", s.handleMCPSessionEvents)
}

// registerTerminalRecordingRoutes mounts the terminal session replay API with
// the same scoping rules as MCP sessions (own sessions for members).
func (s *Server) registerTerminalRecordingRoutes(g fiber.Router) {
	g.Get("/terminal-sessions", s.handleListTerminalSessions)
	g.Get("/terminal-sessions/:id/events", s.handleTerminalSessionEvents)
}

func (s *Server) handleListTerminalSessions(c *fiber.Ctx) error {
	userID, err := s.sessionScope(c)
	if err != nil {
		return err
	}
	limit, offset := pageParams(c, 100, 500)
	q := &domain.TerminalSessionQuery{
		UserID:   userID,
		ServerID: c.Query("server_id"),
		Username: c.Query("username"),
		Limit:    int(limit),
		Offset:   int(offset),
	}
	sessions, total, err := s.db.ListTerminalSessions(q)
	if err != nil {
		return apierr.StorageErr(err)
	}
	return resp.OK(c, fiber.Map{"items": sessions, "total": total})
}

func (s *Server) handleTerminalSessionEvents(c *fiber.Ctx) error {
	userID, err := s.sessionScope(c)
	if err != nil {
		return err
	}
	sess, err := s.db.GetTerminalSession(c.Params("id"))
	if err != nil {
		return apierr.NotFoundf("session not found")
	}
	if userID != "" && sess.UserID != userID {
		return apierr.NotFoundf("session not found")
	}
	events, err := s.db.ListTerminalEvents(sess.ID)
	if err != nil {
		return apierr.StorageErr(err)
	}
	return resp.OK(c, fiber.Map{"session": sess, "events": events})
}

// sessionScope returns the user filter for the caller: "" = all (admin),
// otherwise the caller's own id (audit.view_self level).
func (s *Server) sessionScope(c *fiber.Ctx) (string, error) {
	id := identityFrom(c)
	if !id.HasPermission(domain.PermAuditViewAll) {
		if !id.HasPermission(domain.PermAuditViewSelf) {
			return "", apierr.Forbidden("missing audit permission")
		}
		return id.UserID, nil
	}
	return "", nil
}

func (s *Server) handleListMCPSessions(c *fiber.Ctx) error {
	userID, err := s.sessionScope(c)
	if err != nil {
		return err
	}
	limit, offset := pageParams(c, 100, 500)
	sessions, total, err := s.db.ListMCPSessions(userID, limit, offset)
	if err != nil {
		return apierr.StorageErr(err)
	}
	return resp.OK(c, fiber.Map{"items": sessions, "total": total})
}

func (s *Server) handleMCPSessionEvents(c *fiber.Ctx) error {
	userID, err := s.sessionScope(c)
	if err != nil {
		return err
	}
	events, err := s.db.QuerySessionEvents(c.Params("id"), userID)
	if err != nil {
		return apierr.StorageErr(err)
	}
	if len(events) == 0 {
		return apierr.NotFoundf("session not found")
	}
	return resp.OK(c, events)
}

// pageParams parses limit/offset query params with defaults and caps.
func pageParams(c *fiber.Ctx, defLimit, maxLimit int64) (int64, int64) {
	limit := defLimit
	if v := c.Query("limit"); v != "" {
		if n, perr := strconv.ParseInt(v, 10, 64); perr == nil && n > 0 && n <= maxLimit {
			limit = n
		}
	}
	offset := int64(0)
	if v := c.Query("offset"); v != "" {
		if n, perr := strconv.ParseInt(v, 10, 64); perr == nil && n > 0 {
			offset = n
		}
	}
	return limit, offset
}
