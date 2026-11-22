// Package logging provides the standardized zap logger used across the
// server: human-friendly console output in dev, structured JSON in prod,
// with optional rotating file output.
package logging

import (
	"os"
	"strings"

	"go.uber.org/zap"
	"go.uber.org/zap/zapcore"
	"gopkg.in/natefinch/lumberjack.v2"
)

// FileConfig describes the rotating log file (all zero = stdout only).
type FileConfig struct {
	Path       string
	MaxSizeMB  int
	MaxBackups int
	MaxAgeDays int
}

// New builds the process logger. mode: "dev" (colored console) or
// "prod" (JSON to stdout). Level overrides via SSHIELD_LOG (debug/info/warn/error).
// When fc.Path is set, records are additionally mirrored to the rotating file.
func New(env string, fc FileConfig) *zap.Logger {
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

	opts := []zap.Option{zap.AddCallerSkip(1)}
	if fc.Path != "" {
		sizeMB := fc.MaxSizeMB
		if sizeMB <= 0 {
			sizeMB = 100
		}
		backups := fc.MaxBackups
		if backups <= 0 {
			backups = 7
		}
		ageDays := fc.MaxAgeDays
		if ageDays <= 0 {
			ageDays = 28
		}
		fileSink := zapcore.AddSync(&lumberjack.Logger{
			Filename:   fc.Path,
			MaxSize:    sizeMB,
			MaxBackups: backups,
			MaxAge:     ageDays,
			Compress:   true,
		})
		// The file always gets structured JSON regardless of console encoding.
		fileCore := zapcore.NewCore(
			zapcore.NewJSONEncoder(zap.NewProductionEncoderConfig()),
			fileSink,
			lvl,
		)
		opts = append(opts, zap.WrapCore(func(core zapcore.Core) zapcore.Core {
			return zapcore.NewTee(core, fileCore)
		}))
	}

	logger, err := cfg.Build(opts...)
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
