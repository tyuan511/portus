package auth

import (
	"time"

	"github.com/yuantang/sshield/internal/apierr"
	"github.com/yuantang/sshield/internal/domain"
	"github.com/yuantang/sshield/internal/storage"
)

// CredentialStore is the user/session/token storage surface (storage.DB).
type CredentialStore interface {
	GetCredentialsByUsername(username string) (*storage.UserCredentials, error)
	GetCredentialsByID(id string) (*storage.UserCredentials, error)
	ConsumeRecoveryCode(id string, remaining []string) error
	RecordLoginSuccess(id string) error
	RecordLoginFailure(id string, maxAttempts, lockoutMinutes int) (*time.Time, error)
	SetUserPassword(id, hash string, mustChange bool) error
	SetUserTOTP(id string, secret string, enabled bool, recoveryCodes []string) error
	CreateSession(s *domain.UserSession, tokenHash string, absoluteExpires time.Time) error
	GetSessionByHash(hash string) (*domain.UserSession, error)
	TouchSession(id string, newExpiry time.Time)
	DeleteSession(id string) (bool, error)
	DeleteSessionsForUser(userID string) (int64, error)
	ListSessionsForUser(userID string) ([]*domain.UserSession, error)
	ListAllSessions() ([]*domain.UserSession, error)
	CreateApiToken(t *domain.ApiToken, tokenHash string) error
	GetApiTokenByHash(hash string) (*domain.ApiToken, error)
	TouchApiToken(id string)
	ListApiTokensForUser(userID string) ([]*domain.ApiToken, error)
	RevokeApiToken(id, userID string) (bool, error)
	GetUser(id string) (*domain.User, error)
}

// Service orchestrates the full authentication lifecycle. Identity
// permissions derive from the account's built-in role.
type Service struct {
	store       CredentialStore
	totp        *TOTPManager
	guard       *LoginGuard
	idle        time.Duration
	absolute    time.Duration
	enforceTOTP bool
}

// NewService wires the auth service.
func NewService(store CredentialStore, totpMgr *TOTPManager, guard *LoginGuard,
	idle, absolute time.Duration, enforceTOTP bool) *Service {
	return &Service{
		store: store, totp: totpMgr, guard: guard,
		idle: idle, absolute: absolute, enforceTOTP: enforceTOTP,
	}
}

// LoginResult carries the outcome of a successful login.
type LoginResult struct {
	SessionToken     string
	Session          *domain.UserSession
	Identity         *domain.Identity
	MustChangePass   bool
	RecoveryCodeUsed bool
}

// Login validates credentials and mints a session. totpCode may be empty;
// when the account has TOTP enabled the caller must collect it (or a recovery
// code). Unknown users burn dummy work to defeat timing enumeration.
func (s *Service) Login(username, password, totpCode, ip, userAgent string) (*LoginResult, error) {
	if err := s.guard.Preflight(username, ip); err != nil {
		return nil, err
	}

	creds, err := s.store.GetCredentialsByUsername(username)
	if err != nil {
		DummyVerify()
		s.guard.RecordFailure("", username, ip, 0, nil)
		return nil, apierr.New(apierr.CodeInvalidCredentials, 401, "invalid username or password")
	}
	if !creds.IsActive {
		return nil, apierr.Forbidden("account is disabled")
	}

	ok, err := VerifyPassword(password, creds.PasswordHash)
	if err != nil || !ok {
		lockedUntil, ferr := s.store.RecordLoginFailure(creds.UserID, s.guard.cfg.LoginMaxAttempts, s.guard.cfg.LockoutMinutes)
		if ferr != nil {
			return nil, apierr.StorageErr(ferr)
		}
		if serr := s.guard.RecordFailure(creds.UserID, username, ip, 0, lockedUntil); serr != nil {
			return nil, serr
		}
		return nil, apierr.New(apierr.CodeInvalidCredentials, 401, "invalid username or password")
	}

	// Second factor when enrolled (or globally enforced at enrollment time).
	recoveryUsed := false
	if creds.TOTPEnabled {
		if totpCode == "" {
			return nil, apierr.Locked(apierr.CodeTOTPRequired, "totp_required")
		}
		if s.verifySecondFactor(creds, totpCode) {
			// Accepted (TOTP or recovery code); recovery detection below.
		} else {
			_, ferr := s.store.RecordLoginFailure(creds.UserID, s.guard.cfg.LoginMaxAttempts, s.guard.cfg.LockoutMinutes)
			if ferr != nil {
				return nil, apierr.StorageErr(ferr)
			}
			if serr := s.guard.RecordFailure(creds.UserID, username, ip, 0, nil); serr != nil {
				return nil, serr
			}
			return nil, apierr.New(apierr.CodeTOTPInvalid, 401, "invalid verification code")
		}
		if !s.isTOTPCode(creds, totpCode) {
			recoveryUsed = true
		}
	}

	if err := s.store.RecordLoginSuccess(creds.UserID); err != nil {
		return nil, apierr.StorageErr(err)
	}
	if err := s.guard.RecordSuccess(username, ip); err != nil {
		return nil, apierr.StorageErr(err)
	}

	identity, err := s.buildIdentity(creds)
	if err != nil {
		return nil, err
	}

	now := time.Now().UTC()
	session := &domain.UserSession{
		ID:         "sess-" + NewSessionToken()[:8],
		UserID:     creds.UserID,
		IP:         ip,
		UserAgent:  userAgent,
		CreatedAt:  now,
		LastSeenAt: now,
		ExpiresAt:  now.Add(s.idle),
	}
	token := NewSessionToken()
	if err := s.store.CreateSession(session, HashToken(token), now.Add(s.absolute)); err != nil {
		return nil, apierr.StorageErr(err)
	}

	return &LoginResult{
		SessionToken:     token,
		Session:          session,
		Identity:         identity,
		MustChangePass:   creds.MustChangePassword,
		RecoveryCodeUsed: recoveryUsed,
	}, nil
}

func (s *Service) verifySecondFactor(creds *storage.UserCredentials, code string) bool {
	if raw, ok := s.totp.Unseal(creds.TOTPSecret); ok && s.totp.Validate(raw, code) {
		return true
	}
	// Recovery code fallback: consume on match.
	if idx := MatchRecoveryCode(creds.RecoveryCodes, code); idx >= 0 {
		remaining := append([]string{}, creds.RecoveryCodes[:idx]...)
		remaining = append(remaining, creds.RecoveryCodes[idx+1:]...)
		_ = s.store.ConsumeRecoveryCode(creds.UserID, HashRecoveryCodes(remaining))
		return true
	}
	return false
}

func (s *Service) isTOTPCode(creds *storage.UserCredentials, code string) bool {
	raw, ok := s.totp.Unseal(creds.TOTPSecret)
	return ok && s.totp.Validate(raw, code)
}

func (s *Service) buildIdentity(creds *storage.UserCredentials) (*domain.Identity, error) {
	return identityFor(creds.UserID, creds.Username, creds.DisplayName, creds.Role, creds.TOTPEnabled), nil
}

// ChangePassword verifies the old password, applies policy, rotates the hash,
// and revokes all existing sessions of the user (session-fixation hygiene).
func (s *Service) ChangePassword(userID, username, oldPassword, newPassword string) error {
	creds, err := s.store.GetCredentialsByUsername(username)
	if err != nil {
		return apierr.New(apierr.CodeInvalidCredentials, 401, "invalid username or password")
	}
	if ok, err := VerifyPassword(oldPassword, creds.PasswordHash); err != nil || !ok {
		return apierr.New(apierr.CodeInvalidCredentials, 401, "current password is incorrect")
	}
	if err := CheckPasswordPolicy(newPassword, username); err != nil {
		return err
	}
	if err := s.store.SetUserPassword(userID, HashPassword(newPassword), false); err != nil {
		return apierr.StorageErr(err)
	}
	_, _ = s.store.DeleteSessionsForUser(userID)
	return nil
}

// ResetPasswordByAdmin sets a fresh password (admin flow) and revokes sessions.
func (s *Service) ResetPasswordByAdmin(userID, username, newPassword string) (string, error) {
	if newPassword == "" {
		newPassword = NewSessionToken()[:14]
	} else if err := CheckPasswordPolicy(newPassword, username); err != nil {
		return "", err
	}
	if err := s.store.SetUserPassword(userID, HashPassword(newPassword), true); err != nil {
		return "", apierr.StorageErr(err)
	}
	_, _ = s.store.DeleteSessionsForUser(userID)
	return newPassword, nil
}

// SetupTOTP starts enrollment: returns otpauth URL + plaintext secret.
func (s *Service) SetupTOTP(userID, username string) (secret, url string, err error) {
	secret, url, err = s.totp.Setup(username)
	if err != nil {
		return "", "", err
	}
	if err := s.store.SetUserTOTP(userID, s.totp.Seal(secret), false, nil); err != nil {
		return "", "", apierr.StorageErr(err)
	}
	return secret, url, nil
}

// EnableTOTP confirms enrollment with a live code and issues recovery codes.
func (s *Service) EnableTOTP(userID string, code string) ([]string, error) {
	creds, err := s.store.GetCredentialsByID(userID)
	if err != nil {
		return nil, apierr.NotFoundf("user not found")
	}
	raw, ok := s.totp.Unseal(creds.TOTPSecret)
	if !ok {
		return nil, apierr.Unprocessable("totp enrollment not started")
	}
	if !s.totp.Validate(raw, code) {
		return nil, apierr.New(apierr.CodeTOTPInvalid, 401, "invalid verification code")
	}
	codes := GenerateRecoveryCodes(8)
	if err := s.store.SetUserTOTP(userID, creds.TOTPSecret, true, HashRecoveryCodes(codes)); err != nil {
		return nil, apierr.StorageErr(err)
	}
	return codes, nil
}

// DisableTOTP turns off the second factor after password + code verification.
func (s *Service) DisableTOTP(userID, username, password, code string) error {
	creds, err := s.store.GetCredentialsByUsername(username)
	if err != nil || creds.UserID != userID {
		return apierr.New(apierr.CodeInvalidCredentials, 401, "invalid credentials")
	}
	if ok, err := VerifyPassword(password, creds.PasswordHash); err != nil || !ok {
		return apierr.New(apierr.CodeInvalidCredentials, 401, "password is incorrect")
	}
	if !s.verifySecondFactor(creds, code) {
		return apierr.New(apierr.CodeTOTPInvalid, 401, "invalid verification code")
	}
	if err := s.store.SetUserTOTP(userID, "", false, nil); err != nil {
		return apierr.StorageErr(err)
	}
	return nil
}

// ResolveSession validates a cookie token into a hydrated identity, sliding
// the idle window. Returns nil identity when invalid/expired.
func (s *Service) ResolveSession(token string) (*domain.Identity, *domain.UserSession, error) {
	session, err := s.store.GetSessionByHash(HashToken(token))
	if err != nil {
		return nil, nil, nil
	}
	now := time.Now().UTC()
	if now.After(session.ExpiresAt) {
		_, _ = s.store.DeleteSession(session.ID)
		return nil, nil, nil
	}
	user, err := s.store.GetUser(session.UserID)
	if err != nil || user == nil || !user.IsActive {
		_, _ = s.store.DeleteSession(session.ID)
		return nil, nil, nil
	}
	identity, err := s.identityForUser(user)
	if err != nil {
		return nil, nil, nil
	}
	newExpiry := now.Add(s.idle)
	if absoluteDeadline := session.CreatedAt.Add(s.absolute); newExpiry.After(absoluteDeadline) {
		newExpiry = absoluteDeadline
	}
	s.store.TouchSession(session.ID, newExpiry)
	return identity, session, nil
}

// ResolveApiToken validates a PAT into a hydrated identity.
func (s *Service) ResolveApiToken(token string) (*domain.Identity, *domain.ApiToken, error) {
	record, err := s.store.GetApiTokenByHash(HashToken(token))
	if err != nil {
		return nil, nil, apierr.New(apierr.CodeUnauthorized, 401, "invalid token")
	}
	now := time.Now().UTC()
	if record.RevokedAt != nil {
		return nil, nil, apierr.New(apierr.CodeTokenExpired, 401, "token has been revoked")
	}
	if record.ExpiresAt != nil && now.After(*record.ExpiresAt) {
		return nil, nil, apierr.New(apierr.CodeTokenExpired, 401, "token has expired")
	}
	user, err := s.store.GetUser(record.UserID)
	if err != nil || user == nil || !user.IsActive {
		return nil, nil, apierr.Forbidden("account is disabled")
	}
	identity, err := s.identityForUser(user)
	if err != nil {
		return nil, nil, err
	}
	s.store.TouchApiToken(record.ID)
	return identity, record, nil
}

func (s *Service) identityForUser(user *domain.User) (*domain.Identity, error) {
	return identityFor(user.ID, user.Username, user.DisplayName, user.Role, user.TOTPEnabled), nil
}

// identityFor resolves a built-in role name into a hydrated principal.
func identityFor(userID, username, displayName, role string, totpEnabled bool) *domain.Identity {
	return &domain.Identity{
		UserID:      userID,
		Username:    username,
		DisplayName: displayName,
		Role:        role,
		Permissions: domain.RolePermissions(role),
		IsAdmin:     role == domain.BuiltinRoleAdmin,
		TOTPEnabled: totpEnabled,
	}
}

// EnforceTOTP reports the global enforcement switch (new logins must enroll).
func (s *Service) EnforceTOTP() bool { return s.enforceTOTP }
