// Package vault implements the encrypted credential vault: Argon2id key
// derivation + AES-256-GCM per-credential sealing with idle auto-lock.
//
// Security properties:
//   - The master key is derived from the operator passphrase (Argon2id) and kept
//     only in memory; Lock() and idle timeout wipe it (best-effort in GC runtimes).
//   - Each credential is sealed with a fresh 96-bit nonce; the credential id is
//     bound as AEAD associated data so records cannot be swapped.
//   - The vault file is JSON on disk with 0600 permissions; plaintext secrets
//     never leave this package except through GetSecret.
package vault

import (
	"crypto/aes"
	"crypto/cipher"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"sync"
	"time"

	"golang.org/x/crypto/argon2"

	"github.com/yuantang/sshield/internal/apierr"
	"github.com/yuantang/sshield/internal/domain"
)

const (
	fileVersion  = 1
	kdfAlgorithm = "argon2id"
	// Argon2id parameters: OWASP-aligned, generous because derivation only
	// happens on initialize/unlock (not per credential access).
	argon2Time    = 2
	argon2MemoryK = 64 * 1024 // 64 MiB
	argon2Threads = 2
)

// ErrLocked is returned when an operation needs the master key but the vault
// is locked (or auto-locked after idle).
var ErrLocked = errors.New("vault is locked")

type sealedRecord struct {
	Meta          domain.VaultCredentialMeta `json:"meta"`
	NonceHex      string                     `json:"nonce_hex"`
	CiphertextHex string                     `json:"ciphertext_hex"`
}

type vaultFile struct {
	Version       int                      `json:"version"`
	KDF           string                   `json:"kdf"`
	SaltHex       string                   `json:"salt_hex"`
	VerifierNonce string                   `json:"verifier_nonce_hex"`
	VerifierCT    string                   `json:"verifier_ct_hex"`
	Records       map[string]*sealedRecord `json:"records"`
}

// Vault is the concurrency-safe encrypted credential store.
type Vault struct {
	path       string
	autoLock   time.Duration
	mu         sync.Mutex
	key        []byte // nil = locked
	lastUsed   time.Time
	cachedFile *vaultFile // metadata readable while locked
}

// New builds a vault backed by path with the given idle auto-lock duration
// (<= 0 disables auto-locking).
func New(path string, autoLock time.Duration) *Vault {
	return &Vault{path: path, autoLock: autoLock}
}

// Path returns the vault file location.
func (v *Vault) Path() string { return v.path }

// Status reports the lock state and credential count without exposing secrets.
func (v *Vault) Status() domain.VaultStatus {
	v.mu.Lock()
	defer v.mu.Unlock()

	if _, err := os.Stat(v.path); os.IsNotExist(err) {
		return domain.VaultStatus{State: domain.VaultUninitialized, AutoLockSecs: int64(v.autoLock.Seconds())}
	}
	file, err := v.readFileLocked()
	if err != nil {
		return domain.VaultStatus{State: domain.VaultLocked, AutoLockSecs: int64(v.autoLock.Seconds())}
	}
	v.expireIfIdleLocked()
	state := domain.VaultLocked
	if v.key != nil {
		state = domain.VaultUnlocked
	}
	return domain.VaultStatus{
		State:           state,
		CredentialCount: len(file.Records),
		AutoLockSecs:    int64(v.autoLock.Seconds()),
	}
}

// Initialize creates a brand-new vault sealed with the master passphrase.
func (v *Vault) Initialize(passphrase string) error {
	if len(passphrase) < 8 {
		return apierr.Unprocessable("master passphrase must be at least 8 characters")
	}
	v.mu.Lock()
	defer v.mu.Unlock()

	if _, err := os.Stat(v.path); err == nil {
		return apierr.Conflict("vault is already initialized; unlock instead")
	}
	if err := os.MkdirAll(filepath.Dir(v.path), 0o755); err != nil {
		return apierr.Wrap(apierr.CodeVaultError, 500, "create vault dir", err)
	}

	salt := randomBytes(16)
	key := deriveKey(passphrase, salt)
	vNonce := randomBytes(12)
	ct := seal(key, vNonce, []byte(verifierPlaintext), []byte(verifierAAD))

	file := &vaultFile{
		Version:       fileVersion,
		KDF:           kdfAlgorithm,
		SaltHex:       hex.EncodeToString(salt),
		VerifierNonce: hex.EncodeToString(vNonce),
		VerifierCT:    hex.EncodeToString(ct),
		Records:       map[string]*sealedRecord{},
	}
	if err := v.writeFileLocked(file); err != nil {
		return err
	}
	v.cachedFile = file
	v.setKeyLocked(key)
	return nil
}

// Unlock derives the Argon2id master key and verifies it against the AEAD tag.
func (v *Vault) Unlock(passphrase string) error {
	v.mu.Lock()
	defer v.mu.Unlock()

	if _, err := os.Stat(v.path); os.IsNotExist(err) {
		return apierr.Locked(apierr.CodeVaultError, "vault is not initialized")
	}
	file, err := v.readFileLocked()
	if err != nil {
		return err
	}
	salt, err := hex.DecodeString(file.SaltHex)
	if err != nil {
		return apierr.VaultError("corrupt vault salt")
	}
	key := deriveKey(passphrase, salt)

	vNonce, err1 := hex.DecodeString(file.VerifierNonce)
	vCT, err2 := hex.DecodeString(file.VerifierCT)
	if err1 != nil || err2 != nil {
		return apierr.VaultError("corrupt vault verifier")
	}
	pt, err := open(key, vNonce, vCT, []byte(verifierAAD))
	if err != nil || string(pt) != verifierPlaintext {
		return apierr.Unauthorized("invalid master passphrase")
	}
	v.setKeyLocked(key)
	v.cachedFile = file
	return nil
}

// Lock wipes the master key from memory immediately.
func (v *Vault) Lock() {
	v.mu.Lock()
	defer v.mu.Unlock()
	v.clearKeyLocked()
}

// ListCredentialsMeta returns non-secret metadata (works while locked).
func (v *Vault) ListCredentialsMeta() ([]domain.VaultCredentialMeta, error) {
	v.mu.Lock()
	defer v.mu.Unlock()
	if _, err := os.Stat(v.path); os.IsNotExist(err) {
		return []domain.VaultCredentialMeta{}, nil
	}
	file, err := v.readFileLocked()
	if err != nil {
		return nil, err
	}
	out := make([]domain.VaultCredentialMeta, 0, len(file.Records))
	for _, r := range file.Records {
		out = append(out, r.Meta)
	}
	sort.Slice(out, func(i, j int) bool { return out[i].CreatedAt.After(out[j].CreatedAt) })
	return out, nil
}

// StoreSecret seals and persists a credential.
func (v *Vault) StoreSecret(id, label string, kind domain.VaultCredentialKind, username, secret string) (*domain.VaultCredentialMeta, error) {
	v.mu.Lock()
	defer v.mu.Unlock()

	key, err := v.activeKeyLocked()
	if err != nil {
		return nil, err
	}
	if id == "" {
		id = "cred-" + randomHex(4)
	}

	nonce := randomBytes(12)
	ct := seal(key, nonce, []byte(secret), []byte(id))

	meta := domain.VaultCredentialMeta{
		ID:        id,
		Label:     label,
		Kind:      kind,
		Username:  username,
		Preview:   MaskPreview(secret, kind),
		CreatedAt: time.Now().UTC(),
	}

	file, err := v.readFileLocked()
	if err != nil {
		return nil, err
	}
	file.Records[id] = &sealedRecord{Meta: meta, NonceHex: hex.EncodeToString(nonce), CiphertextHex: hex.EncodeToString(ct)}
	if err := v.writeFileLocked(file); err != nil {
		return nil, err
	}
	v.cachedFile = file
	return &meta, nil
}

// GetSecret decrypts a credential for immediate backend use.
func (v *Vault) GetSecret(id string) (string, error) {
	v.mu.Lock()
	defer v.mu.Unlock()

	key, err := v.activeKeyLocked()
	if err != nil {
		return "", err
	}
	file, err := v.readFileLocked()
	if err != nil {
		return "", err
	}
	rec, ok := file.Records[id]
	if !ok {
		return "", apierr.NotFoundf("credential %s not found", id)
	}
	nonce, err1 := hex.DecodeString(rec.NonceHex)
	ct, err2 := hex.DecodeString(rec.CiphertextHex)
	if err1 != nil || err2 != nil {
		return "", apierr.VaultError("corrupt credential record")
	}
	pt, err := open(key, nonce, ct, []byte(id))
	if err != nil {
		return "", apierr.VaultError("AEAD decryption failed (tampered or wrong key)")
	}
	return string(pt), nil
}

// DeleteSecret removes a credential; returns whether it existed.
func (v *Vault) DeleteSecret(id string) (bool, error) {
	v.mu.Lock()
	defer v.mu.Unlock()

	if _, err := v.activeKeyLocked(); err != nil {
		return false, err
	}
	file, err := v.readFileLocked()
	if err != nil {
		return false, err
	}
	if _, ok := file.Records[id]; !ok {
		return false, nil
	}
	delete(file.Records, id)
	if err := v.writeFileLocked(file); err != nil {
		return false, err
	}
	v.cachedFile = file
	return true, nil
}

// --- internal (mutex held) ---

const (
	verifierAAD       = "sshield-vault-verifier-v1"
	verifierPlaintext = "SSHIELD_VAULT_OK_V1"
)

func (v *Vault) setKeyLocked(key []byte) {
	v.key = key
	v.lastUsed = time.Now()
}

func (v *Vault) clearKeyLocked() {
	if v.key != nil {
		for i := range v.key {
			v.key[i] = 0
		}
	}
	v.key = nil
}

// expireIfIdleLocked applies the idle auto-lock policy.
func (v *Vault) expireIfIdleLocked() {
	if v.key != nil && v.autoLock > 0 && time.Since(v.lastUsed) > v.autoLock {
		v.clearKeyLocked()
	}
}

func (v *Vault) activeKeyLocked() ([]byte, error) {
	v.expireIfIdleLocked()
	if v.key == nil {
		return nil, apierr.Locked(apierr.CodeVaultLocked, "vault is locked; unlock with the master passphrase first")
	}
	v.lastUsed = time.Now()
	// Copy so callers cannot mutate the master key.
	out := make([]byte, len(v.key))
	copy(out, v.key)
	return out, nil
}

func (v *Vault) readFileLocked() (*vaultFile, error) {
	if v.cachedFile != nil {
		return v.cachedFile, nil
	}
	raw, err := os.ReadFile(v.path) // #nosec G304 -- configured vault path
	if err != nil {
		return nil, apierr.Wrap(apierr.CodeVaultError, 500, "read vault file", err)
	}
	var file vaultFile
	if err := json.Unmarshal(raw, &file); err != nil {
		return nil, apierr.VaultError("parse vault file")
	}
	if file.Records == nil {
		file.Records = map[string]*sealedRecord{}
	}
	v.cachedFile = &file
	return v.cachedFile, nil
}

func (v *Vault) writeFileLocked(file *vaultFile) error {
	raw, err := json.MarshalIndent(file, "", "  ")
	if err != nil {
		return apierr.VaultError("serialize vault")
	}
	if err := os.WriteFile(v.path, raw, 0o600); err != nil {
		return apierr.Wrap(apierr.CodeVaultError, 500, "write vault file", err)
	}
	v.cachedFile = file
	return nil
}

// --- crypto helpers ---

func deriveKey(passphrase string, salt []byte) []byte {
	return argon2.IDKey([]byte(passphrase), salt, argon2Time, argon2MemoryK, argon2Threads, 32)
}

func seal(key, nonce, plaintext, aad []byte) []byte {
	block, err := aes.NewCipher(key)
	if err != nil {
		panic(fmt.Sprintf("aes cipher: %v", err)) // 32-byte key cannot fail
	}
	gcm, err := cipher.NewGCM(block)
	if err != nil {
		panic(fmt.Sprintf("gcm: %v", err))
	}
	return gcm.Seal(nil, nonce, plaintext, aad)
}

func open(key, nonce, ciphertext, aad []byte) ([]byte, error) {
	block, err := aes.NewCipher(key)
	if err != nil {
		return nil, err
	}
	gcm, err := cipher.NewGCM(block)
	if err != nil {
		return nil, err
	}
	return gcm.Open(nil, nonce, ciphertext, aad)
}

func randomBytes(n int) []byte {
	b := make([]byte, n)
	if _, err := rand.Read(b); err != nil {
		panic("crypto/rand unavailable: " + err.Error())
	}
	return b
}

func randomHex(bytes int) string { return hex.EncodeToString(randomBytes(bytes)) }

// MaskPreview builds the non-secret preview shown in listings.
func MaskPreview(secret string, kind domain.VaultCredentialKind) string {
	switch kind {
	case domain.CredPassword:
		return "••••••••"
	case domain.CredPrivateKey:
		return fmt.Sprintf("PEM Key (%d bytes)", len(secret))
	default: // api key and anything else
		runes := []rune(secret)
		if len(runes) <= 8 {
			return "***"
		}
		return string(runes[:4]) + "***" + string(runes[len(runes)-4:])
	}
}
