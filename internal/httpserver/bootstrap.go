package httpserver

import (
	"fmt"
	"os"
	"path/filepath"

	"github.com/tyuan511/bulwark/internal/config"
	"github.com/tyuan511/bulwark/internal/domain"
	"github.com/tyuan511/bulwark/internal/security"
	"github.com/tyuan511/bulwark/internal/storage"
)

// Bootstrap seeds the initial admin account (from config or a generated
// password) and the default security rules. It is idempotent: existing
// databases are upgraded in place without touching users.
func Bootstrap(db *storage.DB, cfg *config.Config) (generatedPassword string, err error) {
	// Default L1 rules.
	if n, err := db.CountRules(); err == nil && n == 0 {
		for _, rule := range security.DefaultRules() {
			if err := db.UpsertRule(rule); err != nil {
				return "", fmt.Errorf("seed rules: %w", err)
			}
		}
	}

	// Bootstrap admin only when no users exist at all.
	if n, err := db.CountUsers(); err != nil {
		return "", fmt.Errorf("count users: %w", err)
	} else if n > 0 {
		return "", nil
	}

	username := cfg.Bootstrap.AdminUsername
	if username == "" {
		username = "admin"
	}
	password := cfg.Bootstrap.AdminPassword
	if password == "" && cfg.Bootstrap.AdminPasswordFile != "" {
		if raw, err := os.ReadFile(cfg.Bootstrap.AdminPasswordFile); err == nil { // #nosec G304 -- configured path
			password = string(raw)
		}
	}
	var forcedPassword string
	if password == "" {
		forcedPassword = randomPassword()
		password = forcedPassword
		// Persist once with 0600 so the operator can retrieve it safely.
		_ = os.MkdirAll(cfg.Storage.DataDir, 0o700)
		hintPath := filepath.Join(cfg.Storage.DataDir, "bootstrap-admin-password.txt")
		body := fmt.Sprintf("Initial admin password for %q (delete this file after first login):\n\n%s\n",
			username, password)
		if err := os.WriteFile(hintPath, []byte(body), 0o600); err == nil {
			fmt.Fprintf(os.Stderr, "[bootstrap] initial admin password written to %s\n", hintPath)
		}
		fmt.Fprintf(os.Stderr, "[bootstrap] admin username: %s\n[bootstrap] admin password: %s\n",
			username, password)
	}

	admin := &domain.User{
		ID:          storage.GenerateID("usr"),
		Username:    username,
		DisplayName: "Administrator",
		Role:        domain.BuiltinRoleAdmin,
		IsActive:    true,
		// Forced reset only for generated secrets; an explicitly configured
		// password was chosen by the operator.
		MustChangePassword: forcedPassword != "",
	}
	if err := db.CreateUser(admin, hashPasswordForBootstrap(password), "", nil); err != nil {
		return "", fmt.Errorf("create bootstrap admin: %w", err)
	}
	return forcedPassword, nil
}

// randomPassword produces a memorable-but-strong 20-char password.
func randomPassword() string {
	const alphabet = "abcdefghijkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789"
	out := make([]byte, 20)
	raw := randomBootstrapBytes(len(out))
	for i := range out {
		out[i] = alphabet[int(raw[i])%len(alphabet)]
	}
	return string(out)
}
