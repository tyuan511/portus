package config

import (
	"os"
	"path/filepath"
	"testing"
)

func TestLoadDefaultsWithoutFile(t *testing.T) {
	t.Setenv("SSHIELD_CONFIG", "")
	cfg, err := Load("")
	if err != nil {
		t.Fatalf("load: %v", err)
	}
	if cfg.Server.Bind != "127.0.0.1:8787" {
		t.Fatalf("default bind: %s", cfg.Server.Bind)
	}
	if cfg.Storage.DataDir == "" {
		t.Fatal("data dir must default to user dir")
	}
	if cfg.Security.LoginMaxAttempts != 5 {
		t.Fatalf("login max attempts default: %d", cfg.Security.LoginMaxAttempts)
	}
	if cfg.Storage.DBURL == "" {
		t.Fatal("default postgres DSN must be set")
	}
}

func TestLoadFromYAMLAndEnvOverride(t *testing.T) {
	dir := t.TempDir()
	cfgFile := filepath.Join(dir, "sshield-server.yaml")
	content := `
server:
  bind: "0.0.0.0:9000"

bootstrap:
  admin_username: "root-admin"

storage:
  driver: postgres
  db_url: "postgres://u:p@db.local:5432/sshield"

security:
  login_max_attempts: 3
  lockout_minutes: 30
`
	if err := os.WriteFile(cfgFile, []byte(content), 0o600); err != nil {
		t.Fatal(err)
	}
	t.Setenv("SSHIELD_CONFIG", cfgFile)
	t.Setenv("SSHIELD_STORAGE_DATA_DIR", dir)
	t.Setenv("SSHIELD_SECURITY_LOCKOUT_MINUTES", "42")

	cfg, err := Load("")
	if err != nil {
		t.Fatalf("load: %v", err)
	}
	if cfg.Server.Bind != "0.0.0.0:9000" {
		t.Fatalf("yaml bind not applied: %s", cfg.Server.Bind)
	}
	if cfg.Bootstrap.AdminUsername != "root-admin" {
		t.Fatalf("bootstrap username: %s", cfg.Bootstrap.AdminUsername)
	}
	if cfg.Security.LoginMaxAttempts != 3 {
		t.Fatalf("login attempts: %d", cfg.Security.LoginMaxAttempts)
	}
	if cfg.Security.LockoutMinutes != 42 {
		t.Fatalf("env override failed: %d", cfg.Security.LockoutMinutes)
	}
	if cfg.PostgresDSN() != "postgres://u:p@db.local:5432/sshield" {
		t.Fatalf("dsn: %s", cfg.PostgresDSN())
	}
}

func TestDiscoverTOMLFallback(t *testing.T) {
	dir := t.TempDir()
	t.Setenv("SSHIELD_STORAGE_DATA_DIR", dir)
	t.Setenv("SSHIELD_CONFIG", "")

	cfgFile := filepath.Join(dir, "sshield-server.toml")
	content := `
[server]
bind = "127.0.0.1:9100"
`
	if err := os.WriteFile(cfgFile, []byte(content), 0o600); err != nil {
		t.Fatal(err)
	}
	cfg, err := Load("")
	if err != nil {
		t.Fatalf("load: %v", err)
	}
	if cfg.Server.Bind != "127.0.0.1:9100" {
		t.Fatalf("legacy toml not discovered: %s", cfg.Server.Bind)
	}
}

func TestLegacyEnvOverrides(t *testing.T) {
	t.Setenv("SSHIELD_CONFIG", "")
	t.Setenv("SSHIELD_ADDR", "127.0.0.1:9999")
	t.Setenv("SSHIELD_VAULT_PATH", "/tmp/legacy-vault.enc")
	cfg, err := Load("")
	if err != nil {
		t.Fatalf("load: %v", err)
	}
	if cfg.Server.Bind != "127.0.0.1:9999" {
		t.Fatalf("legacy addr: %s", cfg.Server.Bind)
	}
	if cfg.ServerKeyPath() == "" {
		t.Fatal("server key path must resolve")
	}
}
