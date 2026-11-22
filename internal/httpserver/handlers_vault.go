package httpserver

import (
	"strconv"
	"time"

	"github.com/gofiber/fiber/v2"

	"github.com/yuantang/sshield/internal/apierr"
	"github.com/yuantang/sshield/internal/domain"
	"github.com/yuantang/sshield/internal/httpserver/resp"
	"github.com/yuantang/sshield/internal/storage"
)

// --- security (L1 rules / L2 config / evaluate) ---

func (s *Server) registerSecurityRoutes(g fiber.Router) {
	g.Get("/rules", requirePerm(domain.PermRulesRead), s.handleListRules)
	g.Post("/rules", requirePerm(domain.PermRulesManage), s.handleUpsertRule)
	g.Put("/rules/:id", requirePerm(domain.PermRulesManage), s.handleUpsertRule)
	g.Delete("/rules/:id", requirePerm(domain.PermRulesManage), s.handleDeleteRule)
	g.Get("/l2-config", requirePerm(domain.PermRulesRead), s.handleGetL2)
	g.Put("/l2-config", requirePerm(domain.PermRulesManage), s.handlePutL2)
	g.Post("/security/evaluate", s.handleEvaluate)
}

func (s *Server) handleListRules(c *fiber.Ctx) error {
	rules, err := s.db.ListRules()
	if err != nil {
		return apierr.StorageErr(err)
	}
	return resp.OK(c, rules)
}

func (s *Server) handleUpsertRule(c *fiber.Ctx) error {
	var rule domain.SecurityRule
	if err := c.BodyParser(&rule); err != nil {
		return apierr.BadRequest("invalid JSON body")
	}
	if rule.ID == "" {
		rule.ID = storage.GenerateID("rule")
		rule.Enabled = true // new rules are active unless the payload says otherwise
	}
	if rule.Pattern == "" || rule.Name == "" {
		return apierr.Unprocessable("name and pattern are required")
	}
	if err := s.db.UpsertRule(&rule); err != nil {
		return apierr.StorageErr(err)
	}
	s.InvalidateSecurityCaches()
	s.auditAction(c, "rule.upsert", "upsert rule "+rule.Name, domain.DecisionAllow, "")
	return resp.OK(c, &rule)
}

func (s *Server) handleDeleteRule(c *fiber.Ctx) error {
	ok, err := s.db.DeleteRule(c.Params("id"))
	if err != nil {
		return apierr.StorageErr(err)
	}
	s.InvalidateSecurityCaches()
	return resp.OK(c, fiber.Map{"deleted": ok})
}

func (s *Server) handleGetL2(c *fiber.Ctx) error {
	cfgL2, err := s.db.GetL2Config()
	if err != nil {
		return apierr.StorageErr(err)
	}
	return resp.OK(c, cfgL2)
}

func (s *Server) handlePutL2(c *fiber.Ctx) error {
	var cfgL2 domain.L2ProviderConfig
	if err := c.BodyParser(&cfgL2); err != nil {
		return apierr.BadRequest("invalid JSON body")
	}
	if cfgL2.ID == "" {
		cfgL2.ID = "default-l2"
	}
	if err := s.db.UpsertL2Config(&cfgL2); err != nil {
		return apierr.StorageErr(err)
	}
	s.InvalidateSecurityCaches()
	s.auditAction(c, "l2.upsert", "update L2 provider config", domain.DecisionAllow, "")
	return resp.OK(c, &cfgL2)
}

type evaluateRequest struct {
	Command  string `json:"command"`
	Intent   string `json:"intent"`
	ServerID string `json:"server_id"`
}

func (s *Server) handleEvaluate(c *fiber.Ctx) error {
	var req evaluateRequest
	if err := c.BodyParser(&req); err != nil {
		return apierr.BadRequest("invalid JSON body")
	}
	asset := s.sandboxAsset(req.ServerID)
	verdict := s.engine.Evaluate(req.Command, s.CachedRules())
	var l2 *domain.SemanticReviewVerdict
	if reviewer := s.L2Reviewer(); reviewer != nil {
		v, err := reviewer.Review(c.Context(), &domain.SemanticReviewRequest{
			ServerName: asset.Name,
			ServerTags: asset.Tags,
			Command:    req.Command,
			Intent:     req.Intent,
			L1Verdict:  *verdict,
		})
		if err == nil {
			l2 = v
		}
	}
	final := verdict.Decision
	if l2 != nil {
		final = domain.MoreRestrictive(final, l2.Decision)
	}
	return resp.OK(c, fiber.Map{"l1": verdict, "l2": l2, "final_decision": final})
}

func (s *Server) sandboxAsset(serverID string) *domain.ServerAsset {
	if serverID != "" {
		if asset, err := s.db.GetServer(serverID); err == nil {
			return asset
		}
	}
	return &domain.ServerAsset{ID: "sandbox", Name: "sandbox", Host: "127.0.0.1", Port: 22,
		Username: "ops", AuthKind: domain.AuthAgentOrDefaultKey, Tags: []string{}, Enabled: true}
}

// --- snippets / settings ---

func (s *Server) registerContentRoutes(app *fiber.App, g fiber.Router) {
	g.Get("/snippets", requirePerm(domain.PermSnippetsUse), s.handleListSnippets)
	g.Post("/snippets", requirePerm(domain.PermSnippetsManage), s.handleUpsertSnippet)
	g.Delete("/snippets/:id", requirePerm(domain.PermSnippetsManage), s.handleDeleteSnippet)

	g.Get("/settings/:key", s.handleGetSetting)
	g.Put("/settings/:key", s.handlePutSetting)
}

func (s *Server) handleListSnippets(c *fiber.Ctx) error {
	snippets, err := s.db.ListSnippets()
	if err != nil {
		return apierr.StorageErr(err)
	}
	return resp.OK(c, snippets)
}

func (s *Server) handleUpsertSnippet(c *fiber.Ctx) error {
	var sn domain.CommandSnippet
	if err := c.BodyParser(&sn); err != nil {
		return apierr.BadRequest("invalid JSON body")
	}
	if sn.ID == "" {
		sn.ID = storage.GenerateID("snip")
	}
	if sn.Name == "" || sn.Command == "" {
		return apierr.Unprocessable("name and command are required")
	}
	if err := s.db.UpsertSnippet(&sn); err != nil {
		return apierr.StorageErr(err)
	}
	return resp.OK(c, &sn)
}

func (s *Server) handleDeleteSnippet(c *fiber.Ctx) error {
	ok, err := s.db.DeleteSnippet(c.Params("id"))
	if err != nil {
		return apierr.StorageErr(err)
	}
	return resp.OK(c, fiber.Map{"deleted": ok})
}

// userSettingKey namespaces UI preferences per user while global keys stay
// server-wide (those require settings.manage).
func userSettingKey(id *domain.Identity, key string) (string, bool) {
	if len(key) >= 3 && key[:3] == "ui." {
		return "u:" + id.UserID + ":" + key, true
	}
	return key, false
}

func (s *Server) handleGetSetting(c *fiber.Ctx) error {
	id := identityFrom(c)
	key := c.Params("key")
	dbKey, isUser := userSettingKey(id, key)
	if !isUser {
		if err := s.rbac.CheckPermission(id, domain.PermSettingsManage); err != nil {
			return err
		}
	}
	value, ok, err := s.db.GetSetting(dbKey)
	if err != nil {
		return apierr.StorageErr(err)
	}
	if !ok {
		value = ""
	}
	return resp.OK(c, fiber.Map{"value": value})
}

type settingBody struct {
	Value string `json:"value"`
}

func (s *Server) handlePutSetting(c *fiber.Ctx) error {
	id := identityFrom(c)
	key := c.Params("key")
	dbKey, isUser := userSettingKey(id, key)
	if !isUser {
		if err := s.rbac.CheckPermission(id, domain.PermSettingsManage); err != nil {
			return err
		}
	}
	var body settingBody
	if err := c.BodyParser(&body); err != nil {
		return apierr.BadRequest("invalid JSON body")
	}
	if err := s.db.SetSetting(dbKey, body.Value); err != nil {
		return apierr.StorageErr(err)
	}
	return resp.OK(c, fiber.Map{"value": body.Value})
}

// --- audit logs (scope by permission) ---

func (s *Server) registerAuditRoutes(g fiber.Router) {
	g.Get("/audit-logs", s.handleListAudits)
	g.Get("/audit-logs/stats", s.handleAuditStats)
	g.Get("/audit-logs/verify", requirePerm(domain.PermAuditViewAll), s.handleVerifyChain)
}

func (s *Server) auditScope(c *fiber.Ctx) (*domain.AuditQuery, error) {
	id := identityFrom(c)
	q := &domain.AuditQuery{
		Q:          c.Query("q"),
		ServerID:   c.Query("server_id"),
		SSHUser:    c.Query("ssh_user"),
		Actor:      c.Query("actor"),
		Username:   c.Query("username"),
		Decision:   domain.PolicyDecision(c.Query("decision")),
		OnlyFailed: c.Query("only_failed") == "true",
	}
	if v := c.Query("from"); v != "" {
		if t, err := time.Parse(time.RFC3339, v); err == nil {
			q.From = &t
		}
	}
	if v := c.Query("to"); v != "" {
		if t, err := time.Parse(time.RFC3339, v); err == nil {
			q.To = &t
		}
	}
	if c.Query("limit") != "" {
		if n, err := strconv.Atoi(c.Query("limit")); err == nil {
			q.Limit = n
		}
	}
	if c.Query("offset") != "" {
		if n, err := strconv.Atoi(c.Query("offset")); err == nil {
			q.Offset = n
		}
	}
	// Scope: members see only their own records (self + their MCP activity).
	if !id.HasPermission(domain.PermAuditViewAll) {
		if !id.HasPermission(domain.PermAuditViewSelf) {
			return nil, apierr.Forbidden("missing audit permission")
		}
		q.UserID = id.UserID
		q.Username = ""
	}
	return q, nil
}

func (s *Server) handleListAudits(c *fiber.Ctx) error {
	q, err := s.auditScope(c)
	if err != nil {
		return err
	}
	events, err := s.db.QueryAudits(q)
	if err != nil {
		return apierr.StorageErr(err)
	}
	total, err := s.db.CountAudits(q)
	if err != nil {
		return apierr.StorageErr(err)
	}
	return resp.OK(c, fiber.Map{"items": events, "total": total})
}

func (s *Server) handleAuditStats(c *fiber.Ctx) error {
	q, err := s.auditScope(c)
	if err != nil {
		return err
	}
	filter := &storage.AuditStatsFilter{
		ServerID: q.ServerID,
		UserID:   q.UserID,
	}
	if q.From != nil {
		from := q.From.UTC().Format(time.RFC3339Nano)
		filter.From = &from
	}
	if q.To != nil {
		to := q.To.UTC().Format(time.RFC3339Nano)
		filter.To = &to
	}
	stats, err := s.db.AuditStats(filter)
	if err != nil {
		return apierr.StorageErr(err)
	}
	return resp.OK(c, stats)
}

func (s *Server) handleVerifyChain(c *fiber.Ctx) error {
	status, err := s.db.VerifyAuditChain()
	if err != nil {
		return apierr.StorageErr(err)
	}
	return resp.OK(c, status)
}
