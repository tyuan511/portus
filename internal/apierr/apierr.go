// Package apierr defines the unified business error registry and the
// wire envelope used by every /api endpoint.
//
// Envelope shape:
//
//	{ "code": 0, "message": "ok", "data": <payload>, "trace_id": "..." }
//
// code == 0 means success; non-zero codes are stable, documented identifiers
// grouped by domain (40xxx generic HTTP, 41xxx auth, 42xxx rate limiting,
// 43xxx policy/vault, 44xxx ssh, 50xxx server faults).
package apierr

import (
	"errors"
	"fmt"
	"net/http"
)

// Code is a stable business error code.
type Code int

const (
	CodeOK Code = 0

	// 40xxx — generic request errors.
	CodeInvalidRequest  Code = 40001 // malformed JSON / bad params (400)
	CodeUnauthorized    Code = 40002 // missing or invalid session/token (401)
	CodeForbidden       Code = 40003 // insufficient permission (403)
	CodeNotFound        Code = 40004 // resource not found (404)
	CodeConflict        Code = 40005 // duplicate resource (409)
	CodeValidationFailed Code = 40006 // semantic validation failure (422)
	CodeMethodNotAllowed Code = 40007

	// 41xxx — authentication domain.
	CodeInvalidCredentials Code = 41001 // wrong username/password (401)
	CodeAccountLocked      Code = 41002 // temporarily locked by brute-force guard (423)
	CodeTOTPRequired       Code = 41003 // second factor needed (401)
	CodeTOTPInvalid        Code = 41004 // bad TOTP/recovery code (401)
	CodePasswordPolicy     Code = 41005 // new password rejected (422)
	CodeTokenExpired       Code = 41006 // API token expired/revoked (401)
	CodeAccountDisabled    Code = 41007 // user deactivated by admin (403)

	// 42xxx — rate limiting.
	CodeTooManyAttempts Code = 42001 // login attempts exhausted (429)
	CodeIPBlocked       Code = 42002 // source IP temporarily blocked (429)
	CodeRateLimited     Code = 42003 // generic throttling (429)

	// 43xxx — policy / vault domain.
	CodePolicyDenied   Code = 43001 // security policy denied the command (403)
	CodeParseFailed    Code = 43002 // shell AST parse failed, fail-closed (422)
	CodeL2ReviewFailed Code = 43003 // L2 semantic review error / timeout (503)
	CodeVaultLocked    Code = 43004 // vault must be unlocked first (423)
	CodeVaultError     Code = 43005 // vault operation failed (500)
	CodeApprovalNeeded Code = 43006 // command requires human approval (202-style payload, not an HTTP error)
	CodeApprovalDenied Code = 43007 // approval rejected or timed out (403)

	// 44xxx — SSH domain.
	CodeSSHError        Code = 44001 // generic SSH failure (502)
	CodeSSHAuthFailed   Code = 44002 // remote authentication failed (502)
	CodeSSHTimeout      Code = 44003 // execution timed out (504)
	CodeTerminalFailure Code = 44004 // PTY session could not be established (502)

	// 50xxx — server faults.
	CodeInternal Code = 50001 // unexpected internal error (500)
	CodeStorage   Code = 50002 // persistence layer failure (500)
	CodeConfigErr Code = 50003 // server misconfiguration (500)
)

// E is a structured API error implementing the error interface.
type E struct {
	Code    Code   `json:"code"`
	Status  int    `json:"-"`
	Message string `json:"message"`
	Cause   error  `json:"-"`
}

func (e *E) Error() string {
	if e.Cause != nil {
		return fmt.Sprintf("[%d] %s: %v", e.Code, e.Message, e.Cause)
	}
	return fmt.Sprintf("[%d] %s", e.Code, e.Message)
}

func (e *E) Unwrap() error { return e.Cause }

// New builds a bare structured error.
func New(code Code, status int, message string) *E {
	return &E{Code: code, Status: status, Message: message}
}

// Wrap attaches an underlying cause.
func Wrap(code Code, status int, message string, cause error) *E {
	return &E{Code: code, Status: status, Message: message, Cause: cause}
}

// --- constructors for the common cases ---

func BadRequest(msg string) *E  { return New(CodeInvalidRequest, http.StatusBadRequest, msg) }
func BadRequestf(format string, a ...any) *E {
	return New(CodeInvalidRequest, http.StatusBadRequest, fmt.Sprintf(format, a...))
}
func Unauthorized(msg string) *E { return New(CodeUnauthorized, http.StatusUnauthorized, or(msg, "authentication required")) }
func Forbidden(msg string) *E    { return New(CodeForbidden, http.StatusForbidden, or(msg, "permission denied")) }
func NotFoundf(format string, a ...any) *E {
	return New(CodeNotFound, http.StatusNotFound, fmt.Sprintf(format, a...))
}
func Conflict(msg string) *E { return New(CodeConflict, http.StatusConflict, msg) }
func Unprocessable(msg string) *E { return New(CodeValidationFailed, http.StatusUnprocessableEntity, msg) }
func Locked(code Code, msg string) *E { return New(code, http.StatusLocked, msg) }
func TooManyRequests(code Code, msg string) *E { return New(code, http.StatusTooManyRequests, msg) }
func Internal(msg string, cause error) *E {
	return Wrap(CodeInternal, http.StatusInternalServerError, or(msg, "internal error"), cause)
}
func StorageErr(cause error) *E {
	return Wrap(CodeStorage, http.StatusInternalServerError, "storage error", cause)
}
func VaultError(msg string) *E {
	return New(CodeVaultError, http.StatusInternalServerError, msg)
}
func SSHErrorf(format string, a ...any) *E {
	return New(CodeSSHError, http.StatusBadGateway, fmt.Sprintf(format, a...))
}
func SSHTimeoutf(format string, a ...any) *E {
	return New(CodeSSHTimeout, http.StatusGatewayTimeout, fmt.Sprintf(format, a...))
}

func or(msg, fallback string) string {
	if msg == "" {
		return fallback
	}
	return msg
}

// From extracts an *E from any error, defaulting to a 500 CodeInternal.
func From(err error) *E {
	if err == nil {
		return nil
	}
	var e *E
	if errors.As(err, &e) {
		return e
	}
	return Internal("", err)
}
