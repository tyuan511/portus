// Package sshpool manages cached SSH client connections with ProxyJump
// chains, layered credential resolution (vault → identity file → agent →
// default keys), remote command execution and PTY terminals.
package sshpool

import (
	"fmt"
	"net"
	"os"
	"path/filepath"
	"strings"

	gossh "golang.org/x/crypto/ssh"
	"golang.org/x/crypto/ssh/agent"

	"github.com/tyuan511/bulwark/internal/domain"
)

// SecretResolver resolves vault credentials by id (implemented by vault.Vault).
type SecretResolver interface {
	GetSecret(id string) (string, error)
}

// buildAuthMethods assembles auth methods in priority order:
// vault credential → identity file → ssh agent → default keys.
func buildAuthMethods(asset *domain.ServerAsset, vault SecretResolver) ([]gossh.AuthMethod, error) {
	var methods []gossh.AuthMethod
	var details []string

	// 1. Vault credential: password or PEM private key.
	if asset.CredentialID != "" && vault != nil {
		secret, err := vault.GetSecret(asset.CredentialID)
		if err == nil {
			if strings.Contains(secret, "PRIVATE KEY") {
				if signer, perr := parsePrivateKey(secret); perr == nil {
					methods = append(methods, gossh.PublicKeys(signer))
					details = append(details, "vault private key")
				} else {
					details = append(details, "vault private key (parse failed)")
				}
			} else {
				methods = append(methods, gossh.Password(secret))
				methods = append(methods, gossh.KeyboardInteractive(keyboardInteractive(secret)))
				details = append(details, "vault password")
			}
		} else {
			details = append(details, "vault credential unavailable")
		}
	}

	// 2. Identity file.
	if asset.IdentityFile != "" {
		if signer, err := loadKeyFile(asset.IdentityFile); err == nil {
			methods = append(methods, gossh.PublicKeys(signer))
			details = append(details, "identity file")
		} else {
			details = append(details, "identity file (load failed)")
		}
	}

	// 3. SSH agent.
	if sock := os.Getenv("SSH_AUTH_SOCK"); sock != "" {
		if method, err := agentAuthMethod(sock); err == nil {
			methods = append(methods, method)
			details = append(details, "ssh agent")
		} else {
			details = append(details, "ssh agent (unreachable)")
		}
	}

	// 4. Default keys in ~/.ssh.
	home, _ := os.UserHomeDir()
	if home != "" {
		for _, name := range []string{"id_ed25519", "id_rsa", "id_ecdsa"} {
			path := filepath.Join(home, ".ssh", name)
			if signer, err := loadKeyFile(path); err == nil {
				methods = append(methods, gossh.PublicKeys(signer))
				details = append(details, "default key "+name)
			}
		}
	}

	if len(methods) == 0 {
		return nil, fmt.Errorf("no authentication method available (tried: %s)", strings.Join(details, ", "))
	}
	return methods, nil
}

func parsePrivateKey(pem string) (gossh.Signer, error) {
	signer, err := gossh.ParsePrivateKey([]byte(pem))
	if err == nil {
		return signer, nil
	}
	if _, ok := err.(*gossh.PassphraseMissingError); ok {
		// Vault-stored keys are expected to be unencrypted; surface a clear error.
		return nil, fmt.Errorf("private key is passphrase-protected")
	}
	return nil, err
}

func loadKeyFile(path string) (gossh.Signer, error) {
	raw, err := os.ReadFile(path) // #nosec G304 -- user-configured key path
	if err != nil {
		return nil, err
	}
	return gossh.ParsePrivateKey(raw)
}

func keyboardInteractive(secret string) gossh.KeyboardInteractiveChallenge {
	return func(user, instruction string, questions []string, echos []bool) ([]string, error) {
		answers := make([]string, len(questions))
		for i := range questions {
			answers[i] = secret
		}
		return answers, nil
	}
}

// agentAuthMethod connects to the local SSH agent and signs via its identities.
func agentAuthMethod(sock string) (gossh.AuthMethod, error) {
	conn, err := net.Dial("unix", sock)
	if err != nil {
		return nil, err
	}
	agent := agent.NewClient(conn)
	signers, err := agent.Signers()
	if err != nil {
		_ = conn.Close()
		return nil, err
	}
	if len(signers) == 0 {
		_ = conn.Close()
		return nil, fmt.Errorf("agent has no identities")
	}
	// Keep the agent connection alive for the lifetime of the auth method.
	return gossh.PublicKeysCallback(func() ([]gossh.Signer, error) { return agent.Signers() }), nil
}
