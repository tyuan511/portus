package httpserver

import (
	"time"

	"github.com/gofiber/fiber/v2"

	"github.com/yuantang/sshield/internal/apierr"
	"github.com/yuantang/sshield/internal/auth"
	"github.com/yuantang/sshield/internal/domain"
	"github.com/yuantang/sshield/internal/httpserver/resp"
)

// registerAuthRoutes mounts the authenticated self-service endpoints.
func (s *Server) registerAuthRoutes(g fiber.Router) {
	g.Post("/auth/logout", s.handleLogout)
	g.Get("/auth/me", s.handleMe)
	g.Post("/auth/change-password", s.handleChangePassword)
	g.Post("/auth/totp/setup", s.handleTOTPSetup)
	g.Post("/auth/totp/enable", s.handleTOTPEnable)
	g.Post("/auth/totp/disable", s.handleTOTPDisable)
	g.Get("/auth/sessions", s.handleListMySessions)
	g.Delete("/auth/sessions/:id", s.handleDeleteMySession)
}

func (s *Server) handleHealth(c *fiber.Ctx) error {
	return resp.OK(c, fiber.Map{
		"status":  "ok",
		"service": "sshield-server",
		"time":    time.Now().UTC(),
	})
}

type loginRequest struct {
	Username string `json:"username"`
	Password string `json:"password"`
	TOTPCode string `json:"totp_code"`
}

func (s *Server) handleLogin(c *fiber.Ctx) error {
	var req loginRequest
	if err := c.BodyParser(&req); err != nil {
		return apierr.BadRequest("invalid JSON body")
	}
	if req.Username == "" || req.Password == "" {
		return apierr.BadRequest("username and password are required")
	}

	result, err := s.authSvc.Login(req.Username, req.Password, req.TOTPCode, c.IP(), c.Get("User-Agent"))
	if err != nil {
		// TOTP challenges must be machine-distinguishable for the login UI.
		if e := apierr.From(err); e.Code == apierr.CodeTOTPRequired {
			return c.Status(401).JSON(resp.Envelope{Code: apierr.CodeTOTPRequired, Message: "totp_required"})
		}
		return err
	}

	secure := c.Secure() || s.cfg.Server.TLSKey != ""
	c.Cookie(&fiber.Cookie{
		Name:     auth.SessionCookieName,
		Value:    result.SessionToken,
		Path:     "/",
		HTTPOnly: true,
		Secure:   secure,
		SameSite: "Lax",
		MaxAge:   int(s.cfg.SessionAbsolute().Seconds()),
	})

	s.Audit(&domain.AuditEvent{
		ID:        newAuditID(),
		Actor:     "auth",
		Timestamp: time.Now().UTC(),
		Command:   "login",
		Intent:    "user login",
		Decision:  domain.DecisionAllow,
		UserID:    result.Identity.UserID,
		Username:  result.Identity.Username,
		Operator:  c.IP(),
	})

	return resp.OK(c, fiber.Map{
		"user":                 identityPayload(result.Identity),
		"must_change_password": result.MustChangePass,
		"recovery_code_used":   result.RecoveryCodeUsed,
	})
}

func (s *Server) handleLogout(c *fiber.Ctx) error {
	if token := c.Cookies(auth.SessionCookieName); token != "" {
		if _, session, err := s.authSvc.ResolveSession(token); err == nil && session != nil {
			_, _ = s.db.DeleteSession(session.ID)
		}
	}
	c.ClearCookie(auth.SessionCookieName)
	return resp.OK(c, nil)
}

func (s *Server) handleMe(c *fiber.Ctx) error {
	id := identityFrom(c)
	return resp.OK(c, fiber.Map{"user": identityPayload(id)})
}

type changePasswordRequest struct {
	OldPassword string `json:"old_password"`
	NewPassword string `json:"new_password"`
}

func (s *Server) handleChangePassword(c *fiber.Ctx) error {
	var req changePasswordRequest
	if err := c.BodyParser(&req); err != nil {
		return apierr.BadRequest("invalid JSON body")
	}
	id := identityFrom(c)
	if err := s.authSvc.ChangePassword(id.UserID, id.Username, req.OldPassword, req.NewPassword); err != nil {
		return err
	}
	c.ClearCookie(auth.SessionCookieName)
	return resp.OK(c, fiber.Map{"message": "password changed; sign in again"})
}

// --- TOTP ---

type totpCodeRequest struct {
	Code string `json:"code"`
}

type totpDisableRequest struct {
	Password string `json:"password"`
	Code     string `json:"code"`
}

func (s *Server) handleTOTPSetup(c *fiber.Ctx) error {
	id := identityFrom(c)
	secret, url, err := s.authSvc.SetupTOTP(id.UserID, id.Username)
	if err != nil {
		return err
	}
	return resp.OK(c, fiber.Map{"secret": secret, "otpauth_url": url})
}

func (s *Server) handleTOTPEnable(c *fiber.Ctx) error {
	var req totpCodeRequest
	if err := c.BodyParser(&req); err != nil {
		return apierr.BadRequest("invalid JSON body")
	}
	id := identityFrom(c)
	codes, err := s.authSvc.EnableTOTP(id.UserID, req.Code)
	if err != nil {
		return err
	}
	return resp.OK(c, fiber.Map{"recovery_codes": codes})
}

func (s *Server) handleTOTPDisable(c *fiber.Ctx) error {
	var req totpDisableRequest
	if err := c.BodyParser(&req); err != nil {
		return apierr.BadRequest("invalid JSON body")
	}
	id := identityFrom(c)
	if err := s.authSvc.DisableTOTP(id.UserID, id.Username, req.Password, req.Code); err != nil {
		return err
	}
	return resp.OK(c, nil)
}

// --- own sessions ---

func (s *Server) handleListMySessions(c *fiber.Ctx) error {
	id := identityFrom(c)
	sessions, err := s.db.ListSessionsForUser(id.UserID)
	if err != nil {
		return apierr.StorageErr(err)
	}
	return resp.OK(c, sessions)
}

func (s *Server) handleDeleteMySession(c *fiber.Ctx) error {
	id := identityFrom(c)
	sessionID := c.Params("id")
	// Only allow deleting own sessions.
	sessions, err := s.db.ListSessionsForUser(id.UserID)
	if err != nil {
		return apierr.StorageErr(err)
	}
	own := false
	for _, sess := range sessions {
		if sess.ID == sessionID {
			own = true
			break
		}
	}
	if !own {
		return apierr.NotFoundf("session not found")
	}
	if _, err := s.db.DeleteSession(sessionID); err != nil {
		return apierr.StorageErr(err)
	}
	return resp.OK(c, nil)
}

func identityPayload(id *domain.Identity) fiber.Map {
	return fiber.Map{
		"user_id":      id.UserID,
		"username":     id.Username,
		"display_name": id.DisplayName,
		"role":         id.Role,
		"permissions":  id.Permissions,
		"is_admin":     id.IsAdmin,
		"totp_enabled": id.TOTPEnabled,
	}
}
