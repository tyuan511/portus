// Package logging provides the standardized zap logger used across the
// server: human-friendly console output in dev, structured JSON in prod.
package logging

import (
	"os"
	"strings"

	"go.uber.org/zap"
	"go.uber.org/zap/zapcore"
)

// New builds the process logger. mode: "dev" (colored console) or
// "prod" (JSON to stdout). Level overrides via SSHIELD_LOG (debug/info/warn/error).
func New(env string) *zap.Logger {
	lvl := zapcore.InfoLevel
	switch strings.ToLower(os.Getenv("SSHIELD_LOG")) {
	case "debug":
		lvl = zapcore.DebugLevel
	case "info":
		lvl = zapcore.InfoLevel
	case "warn":
		lvl = zapcore.WarnLevel
	case "error":
		lvl = zapcore.ErrorLevel
	}

	var cfg zap.Config
	if strings.EqualFold(env, "prod") || strings.EqualFold(env, "production") {
		cfg = zap.NewProductionConfig()
	} else {
		cfg = zap.NewDevelopmentConfig()
		cfg.EncoderConfig.EncodeLevel = zapcore.CapitalColorLevelEncoder
	}
	cfg.Level = zap.NewAtomicLevelAt(lvl)

	logger, err := cfg.Build(zap.AddCallerSkip(1))
	if err != nil {
		// Last-resort fallback so the process never runs without logging.
		fallback := zap.NewNop()
		fallback.Error("failed to build zap logger; using nop", zap.Error(err))
		return fallback
	}
	return logger
}

// NewNop returns a no-op logger for tests.
func NewNop() *zap.Logger { return zap.NewNop() }
