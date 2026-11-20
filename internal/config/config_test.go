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
}

func TestLoadFromTOMLAndEnvOverride(t *testing.T) {
	dir := t.TempDir()
	cfgFile := filepath.Join(dir, "sshield-server.toml")
	content := `
[server]
bind = "0.0.0.0:9000"

[bootstrap]
admin_username = "root-admin"

[security]
login_max_attempts = 3
lockout_minutes = 30
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
		t.Fatalf("toml bind not applied: %s", cfg.Server.Bind)
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
	// Relative paths resolve against the config file directory.
	if cfg.DBPath() != filepath.Join(dir, "sshield.db") {
		t.Fatalf("db path: %s", cfg.DBPath())
	}
	if cfg.VaultPath() != filepath.Join(dir, "vault.enc") {
		t.Fatalf("vault path: %s", cfg.VaultPath())
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
	if cfg.VaultPath() != "/tmp/legacy-vault.enc" {
		t.Fatalf("legacy vault path: %s", cfg.VaultPath())
	}
}
