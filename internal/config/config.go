// Package config loads the bulwark-server configuration through viper:
// TOML file + BULWARK_-prefixed environment overrides + typed defaults.
//
// Discovery order: explicit path > $BULWARK_CONFIG > <data_dir>/bulwark-server.toml.
// A missing file is not an error (pure defaults apply).
package config

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/spf13/viper"
)

// Config is the full server configuration.
type Config struct {
	Server        ServerConfig        `mapstructure:"server"`
	Storage       StorageConfig       `mapstructure:"storage"`
	Bootstrap     BootstrapConfig     `mapstructure:"bootstrap"`
	Security      SecurityConfig      `mapstructure:"security"`
	Terminal      TerminalConfig      `mapstructure:"terminal"`
	Retention     RetentionConfig     `mapstructure:"retention"`
	Logging       LoggingConfig       `mapstructure:"logging"`
	Metrics       MetricsConfig       `mapstructure:"metrics"`
	Notifications NotificationsConfig `mapstructure:"notifications"`
}

type ServerConfig struct {
	// Bind is the listen address, e.g. "127.0.0.1:8787".
	Bind string `mapstructure:"bind"`
	// AllowedOrigins enables cross-origin API access; empty = same-origin only.
	AllowedOrigins []string `mapstructure:"allowed_origins"`
	// TLSCert/TLSKey optionally enable built-in HTTPS (recommended: reverse proxy).
	TLSCert string `mapstructure:"tls_cert"`
	TLSKey  string `mapstructure:"tls_key"`
}

type StorageConfig struct {
	// DBURL is the PostgreSQL connection string (pgx URL), e.g.
	// "postgres://bulwark:secret@127.0.0.1:5432/bulwark?sslmode=disable".
	DBURL string `mapstructure:"db_url"`
	// DataDir is the home for the vault, server key and bootstrap hint.
	DataDir string `mapstructure:"data_dir"`
	// AuditJSONL optionally mirrors the audit chain to a JSONL file; empty = off.
	AuditJSONL string `mapstructure:"audit_jsonl"`
}

type TerminalConfig struct {
	// Enforce routes every submitted interactive terminal line through the
	// L1/L2/approval pipeline before it reaches the remote shell.
	// Default: true (commands requiring approval go through approval unless
	// the host grant enables terminal_no_approval).
	Enforce bool `mapstructure:"enforce"`
	// Record captures the full PTY output stream (plus submitted lines) for
	// replay from the Audit page. Default: on.
	Record bool `mapstructure:"record"`
	// RecordMaxBytes caps the recorded output per session; beyond it the
	// recording is truncated with a marker. Default: 10 MiB.
	RecordMaxBytes int64 `mapstructure:"record_max_bytes"`
}

type RetentionConfig struct {
	// MCPSessionDays prunes MCP host-session registry rows older than N days;
	// 0 = keep forever (audit events are never pruned by this).
	MCPSessionDays int `mapstructure:"mcp_session_days"`
	// TerminalDays prunes recorded terminal sessions older than N days;
	// 0 = keep forever.
	TerminalDays int `mapstructure:"terminal_days"`
	// APITokenDays prunes revoked/expired API tokens older than N days;
	// 0 = keep forever.
	APITokenDays int `mapstructure:"api_token_days"`
}

type LoggingConfig struct {
	// File additionally mirrors logs to a rotating file; empty = stdout only.
	File string `mapstructure:"file"`
	// MaxSizeMB per rotated file (default 100), MaxBackups (default 7),
	// MaxAgeDays (default 28).
	MaxSizeMB  int `mapstructure:"max_size_mb"`
	MaxBackups int `mapstructure:"max_backups"`
	MaxAgeDays int `mapstructure:"max_age_days"`
}

type MetricsConfig struct {
	// Enabled exposes GET /metrics in Prometheus text format (default off).
	Enabled bool `mapstructure:"enabled"`
}

type NotificationsConfig struct {
	// WebhookURL receives JSON POSTs for approval requests/decisions,
	// policy denials and account lockouts; empty = disabled.
	WebhookURL string `mapstructure:"webhook_url"`
	// TimeoutSecs bounds each webhook POST (default 5).
	TimeoutSecs int `mapstructure:"timeout_secs"`
}

type BootstrapConfig struct {
	AdminUsername string `mapstructure:"admin_username"`
	// AdminPassword: empty means "generate a random password at first boot",
	// printed to stderr and written to bootstrap-admin-password.txt (0600).
	AdminPassword string `mapstructure:"admin_password"`
	// AdminPasswordFile reads the initial admin password from a file (0600).
	AdminPasswordFile string `mapstructure:"admin_password_file"`
}

type SecurityConfig struct {
	SessionIdleMinutes int64 `mapstructure:"session_idle_minutes"`
	SessionMaxHours    int64 `mapstructure:"session_max_hours"`
	LoginMaxAttempts   int   `mapstructure:"login_max_attempts"`
	LockoutMinutes     int   `mapstructure:"lockout_minutes"`
	IPMaxAttempts      int   `mapstructure:"ip_max_attempts"`
	IPWindowMinutes    int   `mapstructure:"ip_window_minutes"`
	EnforceTOTP        bool  `mapstructure:"enforce_totp"`
	MCPRatePerMin      int   `mapstructure:"mcp_rate_per_min"`
	// MCPSessionTTLMinutes bounds each server-issued host session.
	MCPSessionTTLMinutes int `mapstructure:"mcp_session_ttl_minutes"`
	// TokenTTLHours for new API tokens; 0 = no expiry by default.
	TokenTTLHours int `mapstructure:"token_ttl_hours"`
}

// Defaults registers every supported key with a safe default so that
// environment overrides apply even when the TOML file omits them.
func Defaults(v *viper.Viper) {
	v.SetDefault("server.bind", "127.0.0.1:8787")
	v.SetDefault("server.allowed_origins", []string{})
	v.SetDefault("server.tls_cert", "")
	v.SetDefault("server.tls_key", "")

	v.SetDefault("storage.db_url", "postgres://bulwark:bulwark@127.0.0.1:5432/bulwark?sslmode=disable")
	v.SetDefault("storage.data_dir", defaultDataDir())
	v.SetDefault("storage.audit_jsonl", "")

	v.SetDefault("bootstrap.admin_username", "admin")
	v.SetDefault("bootstrap.admin_password", "")
	v.SetDefault("bootstrap.admin_password_file", "")

	v.SetDefault("security.session_idle_minutes", 720) // 12h
	v.SetDefault("security.session_max_hours", 168)    // 7d absolute
	v.SetDefault("security.login_max_attempts", 5)
	v.SetDefault("security.lockout_minutes", 15)
	v.SetDefault("security.ip_max_attempts", 20)
	v.SetDefault("security.ip_window_minutes", 60)
	v.SetDefault("security.enforce_totp", false)
	v.SetDefault("security.mcp_rate_per_min", 60)
	v.SetDefault("security.mcp_session_ttl_minutes", 30)
	v.SetDefault("security.token_ttl_hours", 0)

	v.SetDefault("terminal.enforce", true)
	v.SetDefault("terminal.record", true)
	v.SetDefault("terminal.record_max_bytes", 10*1024*1024)

	v.SetDefault("retention.mcp_session_days", 0)
	v.SetDefault("retention.terminal_days", 0)
	v.SetDefault("retention.api_token_days", 0)

	v.SetDefault("logging.file", "")
	v.SetDefault("logging.max_size_mb", 100)
	v.SetDefault("logging.max_backups", 7)
	v.SetDefault("logging.max_age_days", 28)

	v.SetDefault("metrics.enabled", false)

	v.SetDefault("notifications.webhook_url", "")
	v.SetDefault("notifications.timeout_secs", 5)
}

// Bare returns the default configuration as a plain struct (tests, tooling).
func Bare() *Config {
	v := viper.New()
	Defaults(v)
	cfg := &Config{}
	_ = v.Unmarshal(cfg)
	return cfg
}

// Load reads the configuration. path empty = $BULWARK_CONFIG = <data_dir>/bulwark-server.toml.
func Load(path string) (*Config, error) {
	v := viper.New()
	Defaults(v)

	v.SetEnvPrefix("BULWARK")
	v.SetEnvKeyReplacer(strings.NewReplacer(".", "_"))
	v.AutomaticEnv()

	// Resolve the effective data dir first (env may redirect it).
	if v.GetString("storage.data_dir") == "" {
		v.Set("storage.data_dir", defaultDataDir())
	}

	discover := path
	if discover == "" {
		discover = os.Getenv("BULWARK_CONFIG")
	}
	if discover == "" {
		// YAML first; fall back to a legacy TOML file when only that exists.
		yamlPath := filepath.Join(v.GetString("storage.data_dir"), "bulwark-server.yaml")
		tomlPath := filepath.Join(v.GetString("storage.data_dir"), "bulwark-server.toml")
		if _, err := os.Stat(yamlPath); err == nil {
			discover = yamlPath
		} else if _, err := os.Stat(tomlPath); err == nil {
			discover = tomlPath
		}
	}
	if _, err := os.Stat(discover); err == nil {
		v.SetConfigFile(discover)
		v.SetConfigType(strings.TrimPrefix(filepath.Ext(discover), "."))
		if err := v.ReadInConfig(); err != nil {
			return nil, fmt.Errorf("read config %s: %w", discover, err)
		}
	}

	// Legacy single-var overrides retained for compatibility.
	if s := os.Getenv("BULWARK_ADDR"); s != "" {
		v.Set("server.bind", s)
	}
	if s := os.Getenv("BULWARK_DB_URL"); s != "" {
		v.Set("storage.db_url", s)
	}
	if s := os.Getenv("BULWARK_AUDIT_JSONL"); s != "" {
		v.Set("storage.audit_jsonl", s)
	}

	cfg := &Config{}
	if err := v.Unmarshal(cfg); err != nil {
		return nil, fmt.Errorf("decode config: %w", err)
	}
	cfg.expandPaths(discover)
	if err := cfg.validate(); err != nil {
		return nil, err
	}
	return cfg, nil
}

// expandPaths resolves relative storage paths against the data dir so that
// everything lives together by default; absolute paths pass through as-is.
// Non-sqlite DSNs (mysql/postgres URLs) are never path-expanded.
func (c *Config) expandPaths(configPath string) {
	c.Storage.DataDir = expandHome(c.Storage.DataDir)
	rel := func(p string) string {
		p = expandHome(p)
		if p == "" || filepath.IsAbs(p) {
			return p
		}
		return filepath.Join(c.Storage.DataDir, p)
	}
	c.Storage.AuditJSONL = rel(c.Storage.AuditJSONL)
	c.Bootstrap.AdminPasswordFile = rel(c.Bootstrap.AdminPasswordFile)
	c.Server.TLSCert = rel(c.Server.TLSCert)
	c.Server.TLSKey = rel(c.Server.TLSKey)
	c.Logging.File = rel(c.Logging.File)
	_ = configPath
}

func (c *Config) validate() error {
	if c.Server.Bind == "" {
		return fmt.Errorf("server.bind must not be empty")
	}
	if c.Storage.DataDir == "" {
		return fmt.Errorf("storage.data_dir must not be empty")
	}
	if c.Storage.DBURL == "" {
		return fmt.Errorf("storage.db_url must not be empty")
	}
	if c.Security.SessionIdleMinutes < 5 {
		return fmt.Errorf("security.session_idle_minutes must be >= 5")
	}
	if c.Security.SessionMaxHours < 1 {
		return fmt.Errorf("security.session_max_hours must be >= 1")
	}
	if c.Security.LoginMaxAttempts < 1 {
		return fmt.Errorf("security.login_max_attempts must be >= 1")
	}
	return nil
}

// PostgresDSN returns the PostgreSQL connection string (pgx URL).
func (c *Config) PostgresDSN() string { return c.Storage.DBURL }

// DataDirPath returns the resolved data directory (vault, server key, hints).
func (c *Config) DataDirPath() string { return c.Storage.DataDir }

// AuditJSONLPath returns the audit mirror path ("" disables mirroring).
func (c *Config) AuditJSONLPath() string { return c.Storage.AuditJSONL }

// ServerKeyPath returns the path of the server-side AEAD key file.
func (c *Config) ServerKeyPath() string {
	return filepath.Join(c.Storage.DataDir, "server.key")
}

// MCPSessionTTL returns the host-session lifetime for MCP agents.
func (c *Config) MCPSessionTTL() time.Duration {
	if c.Security.MCPSessionTTLMinutes <= 0 {
		return 30 * time.Minute
	}
	return time.Duration(c.Security.MCPSessionTTLMinutes) * time.Minute
}

// SessionIdle returns the idle timeout duration.
func (c *Config) SessionIdle() time.Duration {
	return time.Duration(c.Security.SessionIdleMinutes) * time.Minute
}

// SessionAbsolute returns the absolute session lifetime.
func (c *Config) SessionAbsolute() time.Duration {
	return time.Duration(c.Security.SessionMaxHours) * time.Hour
}

func defaultDataDir() string {
	home, err := os.UserHomeDir()
	if err != nil {
		return ".bulwark"
	}
	return filepath.Join(home, ".local", "share", "bulwark")
}

func expandHome(p string) string {
	if p == "~" || strings.HasPrefix(p, "~/") {
		if home, err := os.UserHomeDir(); err == nil {
			return filepath.Join(home, strings.TrimPrefix(p, "~"))
		}
	}
	return p
}
