// Package httpserver assembles the Fiber application: middleware chain,
// route table with RBAC gating, websocket hubs, and the embedded SPA.
package httpserver

import (
	"context"
	"encoding/json"
	"net/http"
	"runtime/debug"
	"strings"
	"sync"
	"time"

	"github.com/gofiber/fiber/v2"
	"github.com/gofiber/fiber/v2/middleware/adaptor"
	"github.com/gofiber/fiber/v2/middleware/cors"
	fiberrecover "github.com/gofiber/fiber/v2/middleware/recover"
	"github.com/gofiber/fiber/v2/middleware/requestid"
	"go.uber.org/zap"

	"github.com/yuantang/sshield/internal/apierr"
	"github.com/yuantang/sshield/internal/approvals"
	"github.com/yuantang/sshield/internal/auth"
	"github.com/yuantang/sshield/internal/config"
	"github.com/yuantang/sshield/internal/domain"
	"github.com/yuantang/sshield/internal/httpserver/resp"
	"github.com/yuantang/sshield/internal/metrics"
	"github.com/yuantang/sshield/internal/notify"
	"github.com/yuantang/sshield/internal/rbac"
	"github.com/yuantang/sshield/internal/security"
	"github.com/yuantang/sshield/internal/sshpool"
	"github.com/yuantang/sshield/internal/storage"
)

// AuditSink persists audit events (storage.DB).
type AuditSink interface {
	AppendAudit(ev *domain.AuditEvent) error
}

// Server owns the HTTP application and every wired subsystem.
type Server struct {
	app     *fiber.App
	log     *zap.Logger
	cfg     *config.Config
	db      *storage.DB
	authSvc *auth.Service
	rbac    *rbac.Service
	pool    *sshpool.Pool
	engine  *security.Engine
	broker  *approvals.Broker
	notify  *notify.Notifier
	mcpNext http.Handler // optional net/http MCP handler mounted at /mcp

	// Security subsystems with light caching.
	rulesMu      sync.Mutex
	rulesCache   []*domain.SecurityRule
	rulesLoaded  time.Time
	l2Config     *domain.L2ProviderConfig
	l2Loaded     time.Time
	auditEnabled bool
}

// New wires the full HTTP server.
func New(log *zap.Logger, cfg *config.Config, db *storage.DB, authSvc *auth.Service,
	rbacSvc *rbac.Service, pool *sshpool.Pool, engine *security.Engine, broker *approvals.Broker,
	notifier *notify.Notifier) *Server {
	s := &Server{
		log: log, cfg: cfg, db: db,
		authSvc: authSvc, rbac: rbacSvc, pool: pool, engine: engine, broker: broker, notify: notifier,
	}

	s.app = fiber.New(fiber.Config{
		AppName:      "sshield-server",
		ErrorHandler: resp.ErrorHandler(log),
		ReadTimeout:  30 * time.Second,
		WriteTimeout: 0, // websockets and long polls must not be cut
		IdleTimeout:  60 * time.Second,
	})

	s.useGlobalMiddleware()
	s.registerRoutes()
	return s
}

// CachedRules returns the L1 rule set with a 10s TTL.
func (s *Server) CachedRules() []*domain.SecurityRule {
	s.rulesMu.Lock()
	defer s.rulesMu.Unlock()
	if s.rulesCache != nil && time.Since(s.rulesLoaded) < 10*time.Second {
		return s.rulesCache
	}
	rules, err := s.db.ListRules()
	if err != nil || len(rules) == 0 {
		if s.rulesCache != nil {
			return s.rulesCache // keep serving the last known set
		}
		rules = security.DefaultRules()
	}
	s.rulesCache = rules
	s.rulesLoaded = time.Now()
	return s.rulesCache
}

// InvalidateSecurityCaches clears rule/L2 caches after admin edits.
func (s *Server) InvalidateSecurityCaches() {
	s.rulesMu.Lock()
	defer s.rulesMu.Unlock()
	s.rulesCache = nil
	s.l2Config = nil
}

// L2Reviewer returns a reviewer for the active L2 config, or nil when disabled.
func (s *Server) L2Reviewer() security.Reviewer {
	s.rulesMu.Lock()
	defer s.rulesMu.Unlock()
	if s.l2Config == nil || time.Since(s.l2Loaded) >= 10*time.Second {
		cfgL2, err := s.db.GetL2Config()
		if err != nil {
			return nil
		}
		s.l2Config = cfgL2
		s.l2Loaded = time.Now()
	}
	if s.l2Config == nil || !s.l2Config.Enabled {
		return nil
	}
	return security.NewL2Reviewer(s.db, *s.l2Config)
}

// SetMCPHandler mounts a net/http handler (the MCP streamable endpoint) at
// /mcp behind bearer-PAT authentication and per-token rate limiting.
func (s *Server) SetMCPHandler(h http.Handler) {
	s.mcpNext = s.mcpGate(h)
}

// mcpGate enforces: Authorization: Bearer <PAT>, active account with
// mcp.access permission, and a per-token sliding-window rate limit.
func (s *Server) mcpGate(next http.Handler) http.Handler {
	type window struct {
		start time.Time
		count int
	}
	var mu sync.Mutex
	buckets := map[string]*window{}
	limit := s.cfg.Security.MCPRatePerMin
	if limit <= 0 {
		limit = 60
	}

	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		token := mcpBearer(r.Header.Get("Authorization"))
		if token == "" {
			mcpPlainError(w, 401, apierr.CodeUnauthorized, "mcp endpoint requires Authorization: Bearer <api token>")
			return
		}
		identity, _, err := s.authSvc.ResolveApiToken(token)
		if err != nil || identity == nil {
			mcpPlainError(w, 401, apierr.CodeUnauthorized, "invalid or expired api token")
			return
		}
		if !identity.HasPermission(domain.PermMCPAccess) {
			mcpPlainError(w, 403, apierr.CodeForbidden, "missing mcp.access permission")
			return
		}

		key := auth.HashToken(token)
		nowT := time.Now()
		mu.Lock()
		bucket, ok := buckets[key]
		if !ok || nowT.Sub(bucket.start) > time.Minute {
			if len(buckets) > 10_000 {
				// Overflow protection: drop idle windows first (they are
				// recreated on demand); clear everything only if the map is
				// still pathological so one attacker cannot reset others.
				for k, b := range buckets {
					if nowT.Sub(b.start) > time.Minute {
						delete(buckets, k)
					}
				}
				if len(buckets) > 50_000 {
					buckets = map[string]*window{}
				}
			}
			bucket = &window{start: nowT}
			buckets[key] = bucket
		}
		bucket.count++
		count := bucket.count
		mu.Unlock()
		if count > limit {
			mcpPlainError(w, 429, apierr.CodeRateLimited, "mcp rate limit exceeded; slow down")
			return
		}

		if s.mcpNext == nil {
			mcpPlainError(w, 503, apierr.CodeInternal, "mcp endpoint not configured")
			return
		}
		next.ServeHTTP(w, r)
	})
}

func mcpBearer(header string) string {
	parts := strings.SplitN(header, " ", 2)
	if len(parts) == 2 && strings.EqualFold(parts[0], "Bearer") {
		return strings.TrimSpace(parts[1])
	}
	return ""
}

func mcpPlainError(w http.ResponseWriter, status int, code apierr.Code, message string) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(map[string]any{"code": code, "message": message})
}

// Listen starts the HTTP(S) server and blocks until the context is cancelled.
func (s *Server) Listen(ctx context.Context) error {
	go func() {
		<-ctx.Done()
		_ = s.app.Shutdown()
	}()
	if s.cfg.Server.TLSCert != "" && s.cfg.Server.TLSKey != "" {
		return s.app.ListenTLS(s.cfg.Server.Bind, s.cfg.Server.TLSCert, s.cfg.Server.TLSKey)
	}
	return s.app.Listen(s.cfg.Server.Bind)
}

// App exposes the fiber app for tests.
func (s *Server) App() *fiber.App { return s.app }

// --- middleware ---

func (s *Server) useGlobalMiddleware() {
	s.app.Use(fiberrecover.New(fiberrecover.Config{EnableStackTrace: false}))
	s.app.Use(requestid.New())
	s.app.Use(func(c *fiber.Ctx) error {
		s.log.Debug("request",
			zap.String("trace_id", localTraceID(c)),
			zap.String("method", c.Method()),
			zap.String("path", c.Path()),
			zap.String("ip", c.IP()),
		)
		return c.Next()
	})
	s.app.Use(securityHeaders())
	if len(s.cfg.Server.AllowedOrigins) > 0 {
		s.app.Use(cors.New(cors.Config{
			AllowOrigins:     strings.Join(s.cfg.Server.AllowedOrigins, ","),
			AllowMethods:     "GET,POST,PUT,PATCH,DELETE,OPTIONS",
			AllowHeaders:     "Origin,Content-Type,Accept,Authorization",
			AllowCredentials: true,
		}))
	} // else: same-origin only, no CORS headers at all (tightest default).
	// Defense-in-depth CSRF: mutating requests must carry a same-origin
	// Origin/Referer header. Session cookies are SameSite=Lax already.
	s.app.Use(originCheck(s.cfg.Server.AllowedOrigins))
}

// localTraceID reads the request id set by the requestid middleware.
func localTraceID(c *fiber.Ctx) string {
	if v, ok := c.Locals("requestid").(string); ok {
		return v
	}
	return ""
}

func securityHeaders() fiber.Handler {
	return func(c *fiber.Ctx) error {
		c.Set("X-Content-Type-Options", "nosniff")
		c.Set("X-Frame-Options", "DENY")
		c.Set("Referrer-Policy", "no-referrer")
		c.Set("X-XSS-Protection", "0")
		if strings.HasPrefix(c.Path(), "/api") {
			c.Set("Cache-Control", "no-store")
		}
		return c.Next()
	}
}

func originCheck(allowed []string) fiber.Handler {
	permuted := map[string]bool{}
	for _, o := range allowed {
		permuted[strings.ToLower(strings.TrimSuffix(strings.TrimSpace(o), "/"))] = true
	}
	return func(c *fiber.Ctx) error {
		switch c.Method() {
		case fiber.MethodGet, fiber.MethodHead, fiber.MethodOptions:
			return c.Next()
		}
		origin := strings.ToLower(strings.TrimSuffix(c.Get("Origin"), "/"))
		if origin == "" {
			origin = originFromReferer(c.Get("Referer"))
		}
		if origin == "" {
			// No Origin header (server-to-server clients, curl with PAT): allow.
			return c.Next()
		}
		if permuted[origin] {
			return c.Next()
		}
		host := c.Hostname()
		scheme := "http"
		if c.Secure() {
			scheme = "https"
		}
		if origin == scheme+"://"+host {
			return c.Next()
		}
		return apierr.Forbidden("cross-origin request rejected")
	}
}

func originFromReferer(referer string) string {
	idx := strings.Index(referer, "//")
	if idx < 0 {
		return ""
	}
	rest := referer[idx+2:]
	if slash := strings.Index(rest, "/"); slash >= 0 {
		return strings.ToLower(rest[:slash])
	}
	return strings.ToLower(rest)
}

// requireAuth resolves the session cookie into an identity or 401s.
func (s *Server) requireAuth() fiber.Handler {
	return func(c *fiber.Ctx) error {
		token := c.Cookies(auth.SessionCookieName)
		if token == "" {
			return apierr.Unauthorized("")
		}
		identity, _, err := s.authSvc.ResolveSession(token)
		if err != nil {
			return err
		}
		if identity == nil {
			return apierr.Unauthorized("")
		}
		c.Locals("identity", identity)
		return c.Next()
	}
}

// requirePerm gates a route behind one RBAC permission.
func requirePerm(perm domain.Permission) fiber.Handler {
	return func(c *fiber.Ctx) error {
		id := identityFrom(c)
		if id == nil {
			return apierr.Unauthorized("")
		}
		if !id.HasPermission(perm) {
			return apierr.Forbidden("missing permission: " + string(perm))
		}
		return c.Next()
	}
}

func identityFrom(c *fiber.Ctx) *domain.Identity {
	id, _ := c.Locals("identity").(*domain.Identity)
	return id
}

// --- route table ---

func (s *Server) registerRoutes() {
	app := s.app

	// MCP endpoint (net/http handler adapted into fiber). The gate answers
	// with a structured error until SetMCPHandler provides the real handler.
	app.All("/mcp", func(c *fiber.Ctx) error {
		defer func() {
			if p := recover(); p != nil {
				s.log.Error("mcp handler panic", zap.Any("panic", p), zap.String("stack", string(debug.Stack())))
			}
		}()
		return adaptor.HTTPHandler(s.mcpGate(s.mcpNext))(c)
	})

	// Health is intentionally unauthenticated (load balancer probes).
	app.Get("/api/health", s.handleHealth)

	// Public auth endpoints (rate limiting + brute-force guard inside).
	app.Post("/api/auth/login", s.handleLogin)

	authGroup := app.Group("/api", s.requireAuth(), s.enforcementGate())
	s.registerAuthRoutes(authGroup)
	s.registerCredentialRoutes(authGroup)
	s.registerServerRoutes(app, authGroup)
	s.registerSecurityRoutes(authGroup)
	s.registerContentRoutes(app, authGroup)
	s.registerAuditRoutes(authGroup)
	s.registerMCPSessionRoutes(authGroup)
	s.registerTerminalRecordingRoutes(authGroup)
	s.registerSFTPRoutes(authGroup)
	s.registerAdminRoutes(authGroup)
	s.registerTokenRoutes(authGroup)
	s.registerWebSockets(app)

	if s.cfg.Metrics.Enabled {
		app.Get("/metrics", adaptor.HTTPHandler(metrics.Handler()))
	}

	// Embedded SPA last: catch-all for client-side routes.
	s.mountSPA(app)
}

// enforcementGate implements the mandatory enrollment gates after login:
// accounts flagged must_change_password may only change their password, and
// when security.enforce_totp is set, unenrolled accounts may only enroll TOTP.
func (s *Server) enforcementGate() fiber.Handler {
	return func(c *fiber.Ctx) error {
		id := identityFrom(c)
		if id == nil {
			return apierr.Unauthorized("")
		}
		path := c.Path()
		gateAllowed := func(patterns ...string) bool {
			for _, p := range patterns {
				if path == p {
					return true
				}
			}
			return false
		}
		if id.MustChangePassword {
			allowed := gateAllowed("/api/auth/me", "/api/auth/logout", "/api/auth/change-password") ||
				strings.HasPrefix(path, "/api/settings/ui.")
			if !allowed {
				return apierr.New(apierr.CodePasswordChangeReq, 403,
					"password change required before accessing this resource")
			}
			return c.Next()
		}
		if s.cfg.Security.EnforceTOTP && !id.TOTPEnabled {
			allowed := gateAllowed("/api/auth/me", "/api/auth/logout", "/api/auth/change-password",
				"/api/auth/totp/setup", "/api/auth/totp/enable") ||
				strings.HasPrefix(path, "/api/settings/ui.")
			if !allowed {
				return apierr.New(apierr.CodeTOTPEnrollReq, 403,
					"TOTP enrollment required before accessing this resource")
			}
		}
		return c.Next()
	}
}

// Audit writes an event, logging (not failing) on persistence errors.
func (s *Server) Audit(ev *domain.AuditEvent) {
	if err := s.db.AppendAudit(ev); err != nil {
		s.log.Warn("audit append failed", zap.String("audit_id", ev.ID), zap.Error(err))
	}
}
