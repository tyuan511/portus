// Package httpserver assembles the Fiber application: middleware chain,
// route table with RBAC gating, websocket hubs, and the embedded SPA.
package httpserver

import (
	"context"
	"net/http"
	"strings"
	"sync"
	"time"

	"github.com/gofiber/fiber/v2"
	"github.com/gofiber/fiber/v2/middleware/adaptor"
	"github.com/gofiber/fiber/v2/middleware/cors"
	"github.com/gofiber/fiber/v2/middleware/recover"
	"github.com/gofiber/fiber/v2/middleware/requestid"
	"go.uber.org/zap"

	"github.com/yuantang/sshield/internal/apierr"
	"github.com/yuantang/sshield/internal/approvals"
	"github.com/yuantang/sshield/internal/auth"
	"github.com/yuantang/sshield/internal/config"
	"github.com/yuantang/sshield/internal/domain"
	"github.com/yuantang/sshield/internal/httpserver/resp"
	"github.com/yuantang/sshield/internal/rbac"
	"github.com/yuantang/sshield/internal/security"
	"github.com/yuantang/sshield/internal/sshpool"
	"github.com/yuantang/sshield/internal/storage"
	"github.com/yuantang/sshield/internal/vault"
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
	mcpNext http.Handler // optional net/http MCP handler mounted at /mcp

	// Security subsystems with light caching.
	sealedVault  *vault.Vault
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
	sealedVault *vault.Vault) *Server {
	s := &Server{
		log: log, cfg: cfg, db: db,
		authSvc: authSvc, rbac: rbacSvc, pool: pool, engine: engine, broker: broker,
		sealedVault: sealedVault,
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

// cachedRules returns the L1 rule set with a 10s TTL.
func (s *Server) cachedRules() []*domain.SecurityRule {
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

// l2Reviewer returns a reviewer for the active L2 config, or nil when disabled.
func (s *Server) l2Reviewer() security.Reviewer {
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
	return security.NewL2Reviewer(s.sealedVault, *s.l2Config)
}

// SetMCPHandler mounts a net/http handler (the MCP streamable endpoint) at /mcp.
func (s *Server) SetMCPHandler(h http.Handler) {
	s.mcpNext = h
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
	s.app.Use(recover.New(recover.Config{EnableStackTrace: false}))
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

	// MCP endpoint (net/http handler adapted into fiber).
	if s.mcpNext != nil {
		app.All("/mcp", adaptor.HTTPHandler(s.mcpNext))
	}

	// Health is intentionally unauthenticated (load balancer probes).
	app.Get("/api/health", s.handleHealth)

	// Public auth endpoints (rate limiting + brute-force guard inside).
	app.Post("/api/auth/login", s.handleLogin)

	authGroup := app.Group("/api", s.requireAuth())
	s.registerAuthRoutes(authGroup)
	s.registerServerRoutes(app, authGroup)
	s.registerVaultRoutes(authGroup)
	s.registerSecurityRoutes(authGroup)
	s.registerContentRoutes(app, authGroup)
	s.registerAuditRoutes(authGroup)
	s.registerAdminRoutes(authGroup)
	s.registerTokenRoutes(authGroup)
	s.registerWebSockets(app)

	// Embedded SPA last: catch-all for client-side routes.
	s.mountSPA(app)
}

// Audit writes an event, logging (not failing) on persistence errors.
func (s *Server) Audit(ev *domain.AuditEvent) {
	if err := s.db.AppendAudit(ev); err != nil {
		s.log.Warn("audit append failed", zap.String("audit_id", ev.ID), zap.Error(err))
	}
}
