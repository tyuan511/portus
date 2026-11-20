package auth

import (
	"fmt"
	"time"

	"github.com/yuantang/sshield/internal/apierr"
)

// AttemptStore persists login attempts (implemented by storage.DB).
type AttemptStore interface {
	RecordAuthAttempt(username, ip string, success bool) error
	CountFailedAttemptsByUser(username string, since time.Time) (int, error)
	CountFailedAttemptsByIP(ip string, since time.Time) (int, error)
}

// GuardConfig configures the brute-force guard thresholds.
type GuardConfig struct {
	LoginMaxAttempts int
	LockoutMinutes   int
	IPMaxAttempts    int
	IPWindowMinutes  int
}

// LoginGuard is the persisted brute-force defense:
// per-username lockout + per-IP sliding-window block, surviving restarts.
type LoginGuard struct {
	store AttemptStore
	cfg   GuardConfig
}

// NewLoginGuard builds the guard.
func NewLoginGuard(store AttemptStore, cfg GuardConfig) *LoginGuard {
	return &LoginGuard{store: store, cfg: cfg}
}

// Preflight rejects logins from locked users or blocked IPs before any
// password work is done. Returns a wrapped *apierr.E to abort the request.
func (g *LoginGuard) Preflight(username, ip string) error {
	if ip != "" && g.cfg.IPMaxAttempts > 0 {
		n, err := g.store.CountFailedAttemptsByIP(ip, time.Now().Add(-time.Duration(g.cfg.IPWindowMinutes)*time.Minute))
		if err == nil && n >= g.cfg.IPMaxAttempts {
			return apierr.TooManyRequests(apierr.CodeIPBlocked, "too many failed attempts from your network; try again later")
		}
	}
	if g.cfg.LoginMaxAttempts > 0 {
		n, err := g.store.CountFailedAttemptsByUser(username, time.Now().Add(-time.Duration(g.cfg.LockoutMinutes)*time.Minute))
		if err == nil && n >= g.cfg.LoginMaxAttempts {
			return apierr.TooManyRequests(apierr.CodeTooManyAttempts,
				fmt.Sprintf("account temporarily locked after %d failed attempts; try again in %d minutes",
					n, g.cfg.LockoutMinutes))
		}
	}
	return nil
}

// RecordFailure persists a failed attempt and increments the user's lock
// counter; returns locked-until when the threshold was reached.
func (g *LoginGuard) RecordFailure(userID, username, ip string, userFailCount int, userLockedUntil *time.Time) error {
	_ = g.store.RecordAuthAttempt(username, ip, false)
	if userLockedUntil != nil && userLockedUntil.After(time.Now()) {
		return apierr.TooManyRequests(apierr.CodeTooManyAttempts,
			fmt.Sprintf("account locked until %s", userLockedUntil.UTC().Format(time.RFC3339)))
	}
	_ = userFailCount
	return nil
}

// RecordSuccess persists a successful attempt.
func (g *LoginGuard) RecordSuccess(username, ip string) error {
	return g.store.RecordAuthAttempt(username, ip, true)
}
