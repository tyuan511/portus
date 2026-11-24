package httpserver

import (
	"crypto/rand"
	"time"

	"github.com/tyuan511/bulwark/internal/auth"
)

func timeNowUTC() time.Time { return time.Now().UTC() }

func randomBootstrapBytes(n int) []byte {
	b := make([]byte, n)
	if _, err := rand.Read(b); err != nil {
		panic("crypto/rand unavailable: " + err.Error())
	}
	return b
}

// hashPasswordForBootstrap re-uses the auth package's Argon2id hashing.
func hashPasswordForBootstrap(password string) string { return auth.HashPassword(password) }
