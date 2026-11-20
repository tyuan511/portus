// sshield-server is the multi-user enterprise control plane:
// HTTP API + websockets + embedded web UI + MCP streamable endpoint.
package main

import (
	"context"
	"fmt"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/yuantang/sshield/internal/apierr"
	"github.com/yuantang/sshield/internal/approvals"
	"github.com/yuantang/sshield/internal/auth"
	"github.com/yuantang/sshield/internal/config"
	"github.com/yuantang/sshield/internal/httpserver"
	"github.com/yuantang/sshield/internal/logging"
	"github.com/yuantang/sshield/internal/mcpserver"
	"github.com/yuantang/sshield/internal/rbac"
	"github.com/yuantang/sshield/internal/security"
	"github.com/yuantang/sshield/internal/serverkey"
	"github.com/yuantang/sshield/internal/sshpool"
	"github.com/yuantang/sshield/internal/storage"
)

func main() {
	if err := run(); err != nil {
		fmt.Fprintf(os.Stderr, "fatal: %v\n", err)
		os.Exit(1)
	}
}

func run() error {
	cfg, err := config.Load("")
	if err != nil {
		return err
	}
	logger := logging.New(os.Getenv("SSHIELD_ENV"))
	defer func() { _ = logger.Sync() }()
	log := logger.Sugar()

	log.Infow("starting sshield-server",
		"bind", cfg.Server.Bind,
		"data_dir", cfg.Storage.DataDir)

	// Storage (PostgreSQL via sqlc-generated queries).
	db, err := storage.Open(cfg.PostgresDSN(), cfg.AuditJSONLPath())
	if err != nil {
		return fmt.Errorf("open storage: %w", err)
	}
	defer db.Close()

	// Bootstrap roles/admin/rules (idempotent).
	generated, err := httpserver.Bootstrap(db, cfg)
	if err != nil {
		return fmt.Errorf("bootstrap: %w", err)
	}
	if generated != "" {
		log.Warnw("bootstrapped initial admin account; password was generated",
			"username", cfg.Bootstrap.AdminUsername)
	}

	// Process-local encryption root (server.key): seals TOTP secrets and
	// per-host SSH credentials stored in PostgreSQL.
	serverKey, err := serverkey.LoadOrCreate(cfg.ServerKeyPath())
	if err != nil {
		return fmt.Errorf("load server key: %w", err)
	}

	// Auth + security + access subsystems.
	totpMgr := auth.NewTOTPManagerWithKey(serverKey, "SSHield")
	if err != nil {
		return fmt.Errorf("init TOTP manager: %w", err)
	}
	guard := auth.NewLoginGuard(db, auth.GuardConfig{
		LoginMaxAttempts: cfg.Security.LoginMaxAttempts,
		LockoutMinutes:   cfg.Security.LockoutMinutes,
		IPMaxAttempts:    cfg.Security.IPMaxAttempts,
		IPWindowMinutes:  cfg.Security.IPWindowMinutes,
	})
	authSvc := auth.NewService(db, totpMgr, guard,
		cfg.SessionIdle(), cfg.SessionAbsolute(), cfg.Security.EnforceTOTP)

	// Credential secrets at rest are sealed with the server key.
	if err := db.SetEncryptionKey(serverKey); err != nil {
		return err
	}

	rbacSvc := rbac.NewService(db)
	pool := sshpool.NewPool(db)
	engine := security.NewEngine()
	broker := approvals.NewBroker()

	server := httpserver.New(logger, cfg, db, authSvc, rbacSvc, pool, engine, broker)

	// MCP endpoint: bearer-PAT gate at the fiber layer; tools resolve identity
	// per call from the request's Authorization header.
	mcpHandler := mcpserver.NewHandler(mcpserver.Deps{
		DB:     db,
		Auth:   authSvc,
		RBAC:   rbacSvc,
		Pool:   pool,
		Engine: engine,
		Broker: broker,
		Rules:  server.CachedRules,
		L2:     server.L2Reviewer,
		Audit:  server.Audit,
		Log:    logger,
	})
	server.SetMCPHandler(mcpHandler)

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()

	// Periodic hygiene: prune expired sessions and stale auth attempts.
	go func() {
		ticker := time.NewTicker(10 * time.Minute)
		defer ticker.Stop()
		for {
			select {
			case <-ctx.Done():
				return
			case <-ticker.C:
				if n, err := db.CleanupExpired(); err == nil && n > 0 {
					log.Infow("expired records cleaned", "removed", n)
				}
			}
		}
	}()

	log.Infow("sshield-server listening",
		"addr", cfg.Server.Bind,
		"mcp", true,
		"tls", cfg.Server.TLSCert != "")
	if err := server.Listen(ctx); err != nil {
		return apierr.Internal("server stopped", err)
	}
	log.Info("shutdown complete")
	return nil
}
