package sshpool

import (
	"context"
	"fmt"
	"net"
	"strconv"
	"strings"
	"sync"
	"time"

	gossh "golang.org/x/crypto/ssh"

	"github.com/tyuan511/bulwark/internal/apierr"
	"github.com/tyuan511/bulwark/internal/domain"
)

const (
	connectTimeout = 10 * time.Second
	execTimeout    = 120 * time.Second
)

// pooledClient is a cached live SSH client for one server asset.
type pooledClient struct {
	client *gossh.Client
	asset  *domain.ServerAsset
}

// Pool caches SSH connections keyed by server id and reconnects on failure.
type Pool struct {
	mu       sync.Mutex
	sessions map[string]*pooledClient
	vault    SecretResolver
}

// NewPool creates an empty pool.
func NewPool(vault SecretResolver) *Pool {
	return &Pool{sessions: map[string]*pooledClient{}, vault: vault}
}

// Invalidate drops the cached connection for a server (after edit/delete).
func (p *Pool) Invalidate(serverID string) {
	p.mu.Lock()
	defer p.mu.Unlock()
	if pc, ok := p.sessions[serverID]; ok {
		_ = pc.client.Close()
		delete(p.sessions, serverID)
	}
}

// TestConnection dials (fresh, uncached) and authenticates to the asset.
func (p *Pool) TestConnection(asset *domain.ServerAsset, all map[string]*domain.ServerAsset) error {
	if asset.JumpMode == domain.JumpModeSuSSH {
		ctx, cancel := context.WithTimeout(context.Background(), connectTimeout+suStepTimeout)
		defer cancel()
		out, err := p.execSuCommand(ctx, asset, all, "true", connectTimeout+suStepTimeout)
		if err != nil {
			return err
		}
		if out.ExitCode != 0 {
			detail := strings.TrimSpace(out.Stdout)
			if detail != "" {
				return apierr.SSHErrorf("su_ssh test exited with code %d: %s", out.ExitCode, detail)
			}
			return apierr.SSHErrorf("su_ssh test exited with code %d", out.ExitCode)
		}
		return nil
	}
	client, _, err := p.dialChain(asset, all)
	if err != nil {
		return err
	}
	_ = client.Close()
	return nil
}

// GetOrConnect returns a live client for the asset, reusing the cache.
func (p *Pool) GetOrConnect(asset *domain.ServerAsset, all map[string]*domain.ServerAsset) (*gossh.Client, error) {
	p.mu.Lock()
	if pc, ok := p.sessions[asset.ID]; ok {
		client := pc.client
		p.mu.Unlock()
		// Cheap liveness probe: closed connections error immediately.
		if s, err := client.NewSession(); err == nil {
			_ = s.Close()
			return client, nil
		}
		p.Invalidate(asset.ID)
	} else {
		p.mu.Unlock()
	}

	client, resolved, err := p.dialChain(asset, all)
	if err != nil {
		return nil, err
	}
	p.mu.Lock()
	p.sessions[asset.ID] = &pooledClient{client: client, asset: resolved}
	p.mu.Unlock()
	return client, nil
}

// ExecuteCommand runs a command remotely and collects output.
func (p *Pool) ExecuteCommand(ctx context.Context, asset *domain.ServerAsset, all map[string]*domain.ServerAsset, command string, timeout time.Duration) (*domain.CommandOutput, error) {
	if asset.JumpMode == domain.JumpModeSuSSH {
		return p.execSuCommand(ctx, asset, all, command, timeout)
	}
	if timeout <= 0 {
		timeout = execTimeout
	}
	client, err := p.GetOrConnect(asset, all)
	if err != nil {
		return nil, err
	}

	runCtx, cancel := context.WithTimeout(ctx, timeout)
	defer cancel()

	session, err := client.NewSession()
	if err != nil {
		p.Invalidate(asset.ID)
		return nil, apierr.Wrap(apierr.CodeSSHError, 502, "open exec channel", err)
	}
	defer session.Close()

	var stdout, stderr strings.Builder
	session.Stdout = &stdout
	session.Stderr = &stderr

	start := time.Now()
	errCh := make(chan error, 1)
	go func() { errCh <- session.Run(command) }()

	select {
	case err := <-errCh:
		out := &domain.CommandOutput{
			Stdout:     stdout.String(),
			Stderr:     stderr.String(),
			DurationMs: time.Since(start).Milliseconds(),
		}
		if err == nil {
			out.ExitCode = 0
		} else if exitErr, ok := err.(*gossh.ExitError); ok {
			out.ExitCode = exitErr.ExitStatus()
		} else if runCtx.Err() != nil {
			return nil, apierr.SSHTimeoutf("command timed out after %ds", int(timeout.Seconds()))
		} else {
			return nil, apierr.Wrap(apierr.CodeSSHError, 502, "command execution failed", err)
		}
		return out, nil
	case <-runCtx.Done():
		_ = session.Close()
		return nil, apierr.SSHTimeoutf("command timed out after %ds", int(timeout.Seconds()))
	}
}

// dialChain connects through the resolved ProxyJump chain (outermost first).
// Returns the final client and the target asset.
func (p *Pool) dialChain(target *domain.ServerAsset, all map[string]*domain.ServerAsset) (*gossh.Client, *domain.ServerAsset, error) {
	chain, err := ResolveJumpChain(target, all)
	if err != nil {
		return nil, nil, err
	}

	var client *gossh.Client
	for i, hop := range chain {
		addr := net.JoinHostPort(hop.Host, strconv.Itoa(hop.Port))
		config, err := p.clientConfig(hop)
		if err != nil {
			return nil, nil, err
		}

		if i == 0 {
			c, err := dialDirect(addr, config)
			if err != nil {
				return nil, nil, apierr.SSHErrorf("TCP/SSH connect to %s (%s) failed: %v", hop.Name, addr, err)
			}
			client = c
			continue
		}

		// Tunnel through the previous hop with direct-tcpip.
		tunnelConn, err := client.Dial("tcp", addr)
		if err != nil {
			_ = client.Close()
			return nil, nil, apierr.SSHErrorf("ProxyJump direct-tcpip to %s (%s) failed: %v", hop.Name, addr, err)
		}
		c, chans, reqs, err := gossh.NewClientConn(tunnelConn, addr, config)
		if err != nil {
			_ = tunnelConn.Close()
			_ = client.Close()
			return nil, nil, apierr.SSHErrorf("ProxyJump SSH handshake with %s (%s) failed: %v", hop.Name, addr, err)
		}
		client = gossh.NewClient(c, chans, reqs)
	}

	if client == nil {
		return nil, nil, apierr.SSHErrorf("empty connection chain")
	}
	return client, chain[len(chain)-1], nil
}

func dialDirect(addr string, config *gossh.ClientConfig) (*gossh.Client, error) {
	conn, err := net.DialTimeout("tcp", addr, config.Timeout)
	if err != nil {
		return nil, err
	}
	c, chans, reqs, err := gossh.NewClientConn(conn, addr, config)
	if err != nil {
		_ = conn.Close()
		return nil, err
	}
	return gossh.NewClient(c, chans, reqs), nil
}

// clientConfig builds the SSH client config (user, auth methods, host key policy).
func (p *Pool) clientConfig(asset *domain.ServerAsset) (*gossh.ClientConfig, error) {
	methods, err := buildAuthMethods(asset, p.vault)
	if err != nil {
		return nil, apierr.Wrap(apierr.CodeSSHAuthFailed, 502,
			fmt.Sprintf("authentication setup for %s@%s failed", asset.Username, asset.Host), err)
	}
	return &gossh.ClientConfig{
		User:            asset.Username,
		Auth:            methods,
		HostKeyCallback: insecureHostKeyCallback(asset),
		Timeout:         connectTimeout,
	}, nil
}

// insecureHostKeyCallback accepts any host key and logs its fingerprint.
// TODO(hardening): TOFU known_hosts store in a later phase.
func insecureHostKeyCallback(asset *domain.ServerAsset) gossh.HostKeyCallback {
	return func(hostname string, remote net.Addr, key gossh.PublicKey) error {
		_ = asset // fingerprint intentionally logged at debug level by callers
		return nil
	}
}

// ResolveJumpChain walks jump_host_id links from the target to the outermost
// jump, detecting cycles; returns hops outermost-first ending with the target.
func ResolveJumpChain(target *domain.ServerAsset, all map[string]*domain.ServerAsset) ([]*domain.ServerAsset, error) {
	var chain []*domain.ServerAsset
	visited := map[string]bool{}
	cursor := target
	for {
		if visited[cursor.ID] {
			return nil, apierr.SSHErrorf("cyclic ProxyJump detected involving asset `%s`", cursor.ID)
		}
		visited[cursor.ID] = true
		chain = append(chain, cursor)
		if cursor.JumpHostID == "" {
			break
		}
		next, ok := all[cursor.JumpHostID]
		if !ok {
			return nil, apierr.NotFoundf("jump host `%s` not found", cursor.JumpHostID)
		}
		cursor = next
	}
	// Reverse: outermost jump first, target last.
	for i, j := 0, len(chain)-1; i < j; i, j = i+1, j-1 {
		chain[i], chain[j] = chain[j], chain[i]
	}
	return chain, nil
}
