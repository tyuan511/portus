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
	limit := int64(100)
	if v := c.Query("limit"); v != "" {
		if n, perr := strconv.ParseInt(v, 10, 64); perr == nil && n > 0 && n <= 500 {
			limit = n
		}
	}
	sessions, err := s.db.ListMCPSessions(userID, limit)
	if err != nil {
		return apierr.StorageErr(err)
	}
	return resp.OK(c, sessions)
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
