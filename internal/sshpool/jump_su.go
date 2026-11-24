package sshpool

import (
	"bytes"
	"context"
	"fmt"
	"io"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/pkg/sftp"
	gossh "golang.org/x/crypto/ssh"

	"github.com/tyuan511/bulwark/internal/apierr"
	"github.com/tyuan511/bulwark/internal/domain"
)

const (
	suBootTimeout     = 15 * time.Second
	suStepTimeout     = 15 * time.Second
	suQuietPromptWait = 500 * time.Millisecond
	suProbeRetryWait  = 2 * time.Second
	targetPassWait    = 10 * time.Second
)

// ptyExpecter reads from a PTY stdout stream via a background goroutine so
// expect operations can enforce timeouts and preserve any trailing bytes for
// subsequent raw streaming.
type ptyExpecter struct {
	ch   chan readChunk
	buf  []byte
	done bool
	err  error
}

type readChunk struct {
	data []byte
	err  error
}

func newPtyExpecter(r io.Reader) *ptyExpecter {
	e := &ptyExpecter{
		ch: make(chan readChunk, 64),
	}
	go func() {
		defer close(e.ch)
		tmp := make([]byte, 16*1024)
		for {
			n, err := r.Read(tmp)
			if n > 0 {
				cp := make([]byte, n)
				copy(cp, tmp[:n])
				e.ch <- readChunk{data: cp}
			}
			if err != nil {
				e.ch <- readChunk{err: err}
				return
			}
		}
	}()
	return e
}

// Read implements io.Reader, draining any buffered bytes before reading new chunks.
func (e *ptyExpecter) Read(p []byte) (int, error) {
	if len(e.buf) > 0 {
		n := copy(p, e.buf)
		e.buf = e.buf[n:]
		return n, nil
	}
	if e.done {
		if e.err != nil {
			return 0, e.err
		}
		return 0, io.EOF
	}
	chunk, ok := <-e.ch
	if !ok {
		e.done = true
		return 0, io.EOF
	}
	if len(chunk.data) > 0 {
		n := copy(p, chunk.data)
		if n < len(chunk.data) {
			e.buf = append(e.buf, chunk.data[n:]...)
		}
		return n, nil
	}
	if chunk.err != nil {
		e.done = true
		e.err = chunk.err
		return 0, chunk.err
	}
	return 0, nil
}

// waitForMarker reads until marker appears in the stream, discarding everything
// up to and including the marker (and any immediate CR/LF right after it).
func (e *ptyExpecter) waitForMarker(ctx context.Context, marker string, timeout time.Duration) error {
	deadline := time.NewTimer(timeout)
	defer deadline.Stop()

	needle := []byte(marker)
	for {
		if idx := bytes.Index(e.buf, needle); idx >= 0 {
			after := idx + len(needle)
			for after < len(e.buf) && (e.buf[after] == '\r' || e.buf[after] == '\n') {
				after++
			}
			e.buf = append([]byte(nil), e.buf[after:]...)
			return nil
		}
		if e.done {
			return fmt.Errorf("stream closed before marker %q (%v)", marker, e.err)
		}
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-deadline.C:
			return fmt.Errorf("timed out waiting for shell marker %q", marker)
		case chunk, ok := <-e.ch:
			if !ok {
				e.done = true
				return fmt.Errorf("stream closed before marker %q", marker)
			}
			if len(chunk.data) > 0 {
				e.buf = append(e.buf, chunk.data...)
			}
			if chunk.err != nil {
				e.done = true
				e.err = chunk.err
			}
		}
	}
}

// runSuExpect drives `LC_ALL=C exec su - <user>` on the jump host PTY until the
// switched user's shell is ready or an error occurs. Using `exec` replaces the
// outer jump-host shell so that when the subsequent `exec ssh ...` exits, the
// SSH session terminates cleanly with EOF and the target command's exit code.
func (e *ptyExpecter) runSuExpect(ctx context.Context, stdin io.Writer, suUser, suPass string) error {
	const preSuMarker = "__SSHIELD_PRE_SU_1____SSHIELD_PRE_SU_2__"
	cmd := "echo __SSHIELD_PRE_SU_1__\"\"__SSHIELD_PRE_SU_2__; LC_ALL=C exec su - " + shellQuote(suUser) + "\n"
	if _, err := io.WriteString(stdin, cmd); err != nil {
		return fmt.Errorf("write su command: %w", err)
	}
	if err := e.waitForMarker(ctx, preSuMarker, suBootTimeout); err != nil {
		return apierr.SSHErrorf("failed to start su to %s: %v", suUser, err)
	}

	deadline := time.NewTimer(suStepTimeout)
	defer deadline.Stop()

	quietTimer := time.NewTimer(suQuietPromptWait)
	defer quietTimer.Stop()

	passwordSent := false
	markerSent := false
	const suMarker = "__SSHIELD_SU_1____SSHIELD_SU_2__"
	const suProbe = "echo __SSHIELD_SU_1__\"\"__SSHIELD_SU_2__\n"

	for {
		raw := string(e.buf)
		text := stripANSI(raw)
		if isAuthFailure(text) {
			return apierr.SSHErrorf("su to %s failed: authentication failed", suUser)
		}
		if markerSent {
			if idx := strings.Index(raw, suMarker); idx >= 0 {
				after := idx + len(suMarker)
				for after < len(e.buf) && (e.buf[after] == '\r' || e.buf[after] == '\n') {
					after++
				}
				e.buf = append([]byte(nil), e.buf[after:]...)
				return nil
			}
		}
		if !passwordSent && isPasswordPrompt(text) {
			if suPass == "" {
				return apierr.SSHErrorf("su to %s requires a password (configure jump_su_credential_id)", suUser)
			}
			e.buf = nil
			if _, err := io.WriteString(stdin, suPass+"\n"); err != nil {
				return fmt.Errorf("write su password: %w", err)
			}
			passwordSent = true
			resetTimer(quietTimer, suQuietPromptWait)
			continue
		}

		if e.done {
			detail := strings.TrimSpace(text)
			if detail != "" {
				return apierr.SSHErrorf("jump host shell closed during su to %s: %s", suUser, detail)
			}
			return apierr.SSHErrorf("jump host shell closed during su to %s", suUser)
		}

		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-deadline.C:
			return apierr.SSHTimeoutf("timed out waiting for su to %s", suUser)
		case <-quietTimer.C:
			if !markerSent && (passwordSent || looksLikePrompt(text) || suPass == "") {
				if _, err := io.WriteString(stdin, suProbe); err != nil {
					return fmt.Errorf("write su readiness probe: %w", err)
				}
				markerSent = true
				resetTimer(quietTimer, suProbeRetryWait)
			} else if markerSent {
				// Re-send the probe in case PAM flushed early TTY input before the shell started.
				if _, err := io.WriteString(stdin, suProbe); err != nil {
					return fmt.Errorf("write su readiness probe: %w", err)
				}
				resetTimer(quietTimer, suProbeRetryWait)
			} else {
				resetTimer(quietTimer, suQuietPromptWait)
			}
		case chunk, ok := <-e.ch:
			if !ok {
				e.done = true
				detail := strings.TrimSpace(stripANSI(string(e.buf)))
				if isAuthFailure(detail) {
					return apierr.SSHErrorf("su to %s failed: authentication failed", suUser)
				}
				if detail != "" {
					return apierr.SSHErrorf("jump host shell closed during su to %s: %s", suUser, detail)
				}
				return apierr.SSHErrorf("jump host shell closed during su to %s", suUser)
			}
			if len(chunk.data) > 0 {
				e.buf = append(e.buf, chunk.data...)
				stripped := stripANSI(string(e.buf))
				if !markerSent && !isPasswordPrompt(stripped) && looksLikePrompt(stripped) {
					if _, err := io.WriteString(stdin, suProbe); err != nil {
						return fmt.Errorf("write su readiness probe: %w", err)
					}
					markerSent = true
					resetTimer(quietTimer, suProbeRetryWait)
				} else if !markerSent {
					resetTimer(quietTimer, suQuietPromptWait)
				}
			}
			if chunk.err != nil {
				e.done = true
				e.err = chunk.err
			}
		}
	}
}

// maybeInjectTargetPassword watches briefly for an SSH password prompt after
// launching `ssh ...` on the jump host and writes targetPass if prompted.
// If stripPostAuthNewline is true (used for binary SFTP streams), any CR/LF
// echoed by ssh's readpassphrase() immediately after password entry is drained.
func (e *ptyExpecter) maybeInjectTargetPassword(ctx context.Context, stdin io.Writer, targetPass string, stripPostAuthNewline bool) error {
	if targetPass == "" {
		return nil
	}
	timer := time.NewTimer(targetPassWait)
	defer timer.Stop()

	for {
		text := stripANSI(string(e.buf))
		if isPasswordPrompt(text) {
			e.buf = nil
			if _, err := io.WriteString(stdin, targetPass+"\n"); err != nil {
				return err
			}
			if stripPostAuthNewline {
				return e.drainLeadingNewlines(ctx, targetPassWait)
			}
			return nil
		}
		// If binary SFTP packet or target shell prompt already arrived, no password needed.
		if len(e.buf) > 0 && (looksLikePrompt(text) || bytes.Contains(e.buf, []byte{0x00, 0x00, 0x00})) {
			return nil
		}
		if e.done {
			return nil
		}
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-timer.C:
			return nil
		case chunk, ok := <-e.ch:
			if !ok {
				e.done = true
				return nil
			}
			if len(chunk.data) > 0 {
				e.buf = append(e.buf, chunk.data...)
			}
			if chunk.err != nil {
				e.done = true
				e.err = chunk.err
			}
		}
	}
}

// drainLeadingNewlines consumes CR/LF bytes emitted by ssh's readpassphrase()
// right after password submission so subsequent binary streams (SFTP) stay aligned.
func (e *ptyExpecter) drainLeadingNewlines(ctx context.Context, timeout time.Duration) error {
	timer := time.NewTimer(timeout)
	defer timer.Stop()

	for {
		e.buf = bytes.TrimLeft(e.buf, "\r\n")
		if len(e.buf) > 0 || e.done {
			return nil
		}
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-timer.C:
			return nil
		case chunk, ok := <-e.ch:
			if !ok {
				e.done = true
				return nil
			}
			if len(chunk.data) > 0 {
				e.buf = append(e.buf, chunk.data...)
			}
			if chunk.err != nil {
				e.done = true
				e.err = chunk.err
			}
		}
	}
}

// readExecOutput reads all command output until EOF, injecting targetPass if
// ssh prompts for a password along the way and stripping the prompt + echo newline.
func (e *ptyExpecter) readExecOutput(ctx context.Context, stdin io.Writer, targetPass string) (string, error) {
	if targetPass == "" {
		data, err := io.ReadAll(e)
		return string(data), err
	}

	var out bytes.Buffer
	var pending bytes.Buffer
	passwordInjected := false
	trimLeadingNL := false

	processChunk := func(data []byte) error {
		if len(data) == 0 {
			return nil
		}
		if !passwordInjected {
			pending.Write(data)
			if isPasswordPrompt(stripANSI(pending.String())) {
				if _, err := io.WriteString(stdin, targetPass+"\n"); err != nil {
					return fmt.Errorf("write target ssh password: %w", err)
				}
				passwordInjected = true
				trimLeadingNL = true
				pending.Reset()
			}
			return nil
		}
		if trimLeadingNL {
			data = bytes.TrimLeft(data, "\r\n")
			if len(data) == 0 {
				return nil
			}
			trimLeadingNL = false
		}
		out.Write(data)
		return nil
	}

	if len(e.buf) > 0 {
		initial := e.buf
		e.buf = nil
		if err := processChunk(initial); err != nil {
			return "", err
		}
	}

	for !e.done {
		select {
		case <-ctx.Done():
			return "", ctx.Err()
		case chunk, ok := <-e.ch:
			if !ok {
				e.done = true
				break
			}
			if len(chunk.data) > 0 {
				if err := processChunk(chunk.data); err != nil {
					return "", err
				}
			}
			if chunk.err != nil {
				e.done = true
				e.err = chunk.err
			}
		}
	}

	if !passwordInjected && pending.Len() > 0 {
		out.Write(pending.Bytes())
	}
	if e.err != nil && e.err != io.EOF {
		return out.String(), e.err
	}
	return out.String(), nil
}

func resetTimer(t *time.Timer, d time.Duration) {
	if !t.Stop() {
		select {
		case <-t.C:
		default:
		}
	}
	t.Reset(d)
}

// stripANSI removes ANSI CSI/OSC escape sequences (colors, bracketed paste
// mode `\x1b[?2004h`, window titles, etc.) so prompt and error matchers work
// reliably on modern shells.
func stripANSI(s string) string {
	if !strings.ContainsRune(s, '\x1b') {
		return s
	}
	var b strings.Builder
	b.Grow(len(s))
	for i := 0; i < len(s); {
		if s[i] == '\x1b' && i+1 < len(s) {
			next := s[i+1]
			switch next {
			case '[': // CSI sequence: ESC [ ... final byte 0x40..0x7E
				i += 2
				for i < len(s) && (s[i] < 0x40 || s[i] > 0x7e) {
					i++
				}
				if i < len(s) {
					i++
				}
				continue
			case ']': // OSC sequence: ESC ] ... BEL or ESC \
				i += 2
				for i < len(s) {
					if s[i] == '\x07' {
						i++
						break
					}
					if s[i] == '\x1b' && i+1 < len(s) && s[i+1] == '\\' {
						i += 2
						break
					}
					i++
				}
				continue
			default:
				i += 2
				continue
			}
		}
		b.WriteByte(s[i])
		i++
	}
	return b.String()
}

func isPasswordPrompt(s string) bool {
	lower := strings.ToLower(strings.TrimSpace(stripANSI(s)))
	if lower == "" {
		return false
	}
	if idx := strings.LastIndexAny(lower, "\r\n"); idx >= 0 {
		lower = strings.TrimSpace(lower[idx+1:])
	}
	return strings.Contains(lower, "password:") ||
		strings.Contains(lower, "passphrase") ||
		strings.Contains(lower, "密码：") ||
		strings.Contains(lower, "密码:")
}

func isAuthFailure(s string) bool {
	lower := strings.ToLower(stripANSI(s))
	return strings.Contains(lower, "authentication failure") ||
		strings.Contains(lower, "incorrect password") ||
		strings.Contains(lower, "su: sorry") ||
		strings.Contains(lower, "su: failed") ||
		strings.Contains(lower, "permission denied") ||
		strings.Contains(lower, "认证失败")
}

func looksLikePrompt(s string) bool {
	trimmed := strings.TrimRight(stripANSI(s), " \t")
	if trimmed == "" {
		return false
	}
	last := trimmed[len(trimmed)-1]
	return last == '$' || last == '#' || last == '>' || last == '%'
}

func shellQuote(s string) string {
	return "'" + strings.ReplaceAll(s, "'", `'\''`) + "'"
}

func buildTargetSSHCommand(asset *domain.ServerAsset, extraArgs []string, remoteCmd string) string {
	port := asset.Port
	if port <= 0 || port > 65535 {
		port = 22
	}
	parts := []string{
		"ssh",
		"-o", "StrictHostKeyChecking=no",
		"-o", "ConnectTimeout=10",
		"-p", strconv.Itoa(port),
	}
	if asset.AuthKind == domain.AuthIdentityFile && asset.IdentityFile != "" {
		parts = append(parts, "-i", asset.IdentityFile)
	}
	parts = append(parts, extraArgs...)
	parts = append(parts, asset.Username+"@"+asset.Host)
	if remoteCmd != "" {
		parts = append(parts, remoteCmd)
	}
	quoted := make([]string, len(parts))
	for i, p := range parts {
		quoted[i] = shellQuote(p)
	}
	return strings.Join(quoted, " ")
}

func (p *Pool) resolveTargetPassword(asset *domain.ServerAsset) string {
	if asset.CredentialID == "" || p.vault == nil {
		return ""
	}
	secret, err := p.vault.GetSecret(asset.CredentialID)
	if err != nil || strings.Contains(secret, "PRIVATE KEY") {
		return ""
	}
	return secret
}

func (p *Pool) resolveSuPassword(asset *domain.ServerAsset) (string, error) {
	if asset.JumpSuCredentialID == "" || p.vault == nil {
		return "", nil
	}
	secret, err := p.vault.GetSecret(asset.JumpSuCredentialID)
	if err != nil {
		return "", apierr.SSHErrorf("resolve su credential %s: %v", asset.JumpSuCredentialID, err)
	}
	return secret, nil
}

// bootstrapSuShell connects to asset.JumpHostID, opens a PTY shell, drains the
// login banner, and optionally runs `su - <jump_su_username>` with expect.
func (p *Pool) bootstrapSuShell(ctx context.Context, asset *domain.ServerAsset, all map[string]*domain.ServerAsset, cols, rows uint32) (*gossh.Session, io.WriteCloser, *ptyExpecter, error) {
	if asset.JumpHostID == "" {
		return nil, nil, nil, apierr.BadRequest("jump_host_id is required for su_ssh mode")
	}
	if asset.JumpHostID == asset.ID {
		return nil, nil, nil, apierr.SSHErrorf("cyclic jump_host_id involving asset `%s`", asset.ID)
	}
	jumpHost, ok := all[asset.JumpHostID]
	if !ok {
		return nil, nil, nil, apierr.NotFoundf("jump host `%s` not found", asset.JumpHostID)
	}

	client, err := p.GetOrConnect(jumpHost, all)
	if err != nil {
		return nil, nil, nil, err
	}

	session, err := client.NewSession()
	if err != nil {
		p.Invalidate(jumpHost.ID)
		return nil, nil, nil, apierr.Wrap(apierr.CodeSSHError, 502, "open jump host PTY session", err)
	}

	if cols == 0 {
		cols = 120
	}
	if rows == 0 {
		rows = 36
	}
	if err := session.RequestPty("xterm-256color", int(rows), int(cols), gossh.TerminalModes{
		gossh.ECHO:          1,
		gossh.TTY_OP_ISPEED: 14400,
		gossh.TTY_OP_OSPEED: 14400,
	}); err != nil {
		_ = session.Close()
		return nil, nil, nil, apierr.Wrap(apierr.CodeSSHError, 502, "request jump host PTY", err)
	}

	stdin, err := session.StdinPipe()
	if err != nil {
		_ = session.Close()
		return nil, nil, nil, apierr.Wrap(apierr.CodeSSHError, 502, "jump host stdin pipe", err)
	}
	stdout, err := session.StdoutPipe()
	if err != nil {
		_ = session.Close()
		return nil, nil, nil, apierr.Wrap(apierr.CodeSSHError, 502, "jump host stdout pipe", err)
	}

	if err := session.Shell(); err != nil {
		_ = session.Close()
		return nil, nil, nil, apierr.Wrap(apierr.CodeSSHError, 502, "start jump host shell", err)
	}

	exp := newPtyExpecter(stdout)
	if _, err := io.WriteString(stdin, "echo __SSHIELD_BOOT_1__\"\"__SSHIELD_BOOT_2__\n"); err != nil {
		_ = session.Close()
		return nil, nil, nil, apierr.Wrap(apierr.CodeSSHError, 502, "probe jump host shell", err)
	}
	if err := exp.waitForMarker(ctx, "__SSHIELD_BOOT_1____SSHIELD_BOOT_2__", suBootTimeout); err != nil {
		_ = session.Close()
		return nil, nil, nil, apierr.Wrap(apierr.CodeSSHError, 502, "jump host shell not ready", err)
	}

	if asset.JumpSuUsername != "" {
		suPass, err := p.resolveSuPassword(asset)
		if err != nil {
			_ = session.Close()
			return nil, nil, nil, err
		}
		if err := exp.runSuExpect(ctx, stdin, asset.JumpSuUsername, suPass); err != nil {
			_ = session.Close()
			return nil, nil, nil, err
		}
	}

	return session, stdin, exp, nil
}

func (p *Pool) openSuTerminal(ctx context.Context, asset *domain.ServerAsset, all map[string]*domain.ServerAsset, cols, rows uint32) (*TerminalSession, error) {
	session, stdin, exp, err := p.bootstrapSuShell(ctx, asset, all, cols, rows)
	if err != nil {
		return nil, err
	}

	const preSSHMarker = "__SSHIELD_SSH_1____SSHIELD_SSH_2__"
	sshCmd := "echo __SSHIELD_SSH_1__\"\"__SSHIELD_SSH_2__; exec " + buildTargetSSHCommand(asset, []string{"-tt"}, "") + "\n"
	if _, err := io.WriteString(stdin, sshCmd); err != nil {
		_ = session.Close()
		return nil, apierr.Wrap(apierr.CodeTerminalFailure, 502, "launch target ssh", err)
	}
	if err := exp.waitForMarker(ctx, preSSHMarker, suBootTimeout); err != nil {
		_ = session.Close()
		return nil, apierr.Wrap(apierr.CodeTerminalFailure, 502, "wait for target ssh launch", err)
	}

	if targetPass := p.resolveTargetPassword(asset); targetPass != "" {
		if err := exp.maybeInjectTargetPassword(ctx, stdin, targetPass, false); err != nil {
			_ = session.Close()
			return nil, apierr.Wrap(apierr.CodeTerminalFailure, 502, "target ssh password injection", err)
		}
	}

	return bridgeTerminalSession(ctx, session, stdin, exp), nil
}

func (p *Pool) execSuCommand(ctx context.Context, asset *domain.ServerAsset, all map[string]*domain.ServerAsset, command string, timeout time.Duration) (*domain.CommandOutput, error) {
	if timeout <= 0 {
		timeout = execTimeout
	}
	runCtx, cancel := context.WithTimeout(ctx, timeout)
	defer cancel()

	session, stdin, exp, err := p.bootstrapSuShell(runCtx, asset, all, 80, 24)
	if err != nil {
		return nil, err
	}
	defer session.Close()

	start := time.Now()
	targetPass := p.resolveTargetPassword(asset)
	extraArgs := []string{"-T"}
	if targetPass == "" {
		// Non-interactive command execution cannot prompt for a password; fail fast
		// instead of hanging on a password prompt until the command timeout.
		extraArgs = append(extraArgs, "-o", "BatchMode=yes")
	} else {
		// Fail fast if the configured target password is wrong instead of hanging
		// on a second password prompt.
		extraArgs = append(extraArgs, "-o", "NumberOfPasswordPrompts=1")
	}

	// Switch to raw -echo, emit a sync marker, and immediately `exec ssh` in a
	// single shell command line so the jump host shell never emits another prompt
	// that could pollute stdout or prematurely satisfy password expecters.
	const rawMarker = "__SSHIELD_RAW_1____SSHIELD_RAW_2__"
	sshCmd := "stty raw -echo; echo __SSHIELD_RAW_1__\"\"__SSHIELD_RAW_2__; exec " + buildTargetSSHCommand(asset, extraArgs, command) + "\n"
	if _, err := io.WriteString(stdin, sshCmd); err != nil {
		return nil, apierr.Wrap(apierr.CodeSSHError, 502, "launch target ssh exec", err)
	}
	if err := exp.waitForMarker(runCtx, rawMarker, suBootTimeout); err != nil {
		return nil, apierr.Wrap(apierr.CodeSSHError, 502, "enter raw mode on jump host", err)
	}

	type execResult struct {
		stdout string
		err    error
	}
	resCh := make(chan execResult, 1)
	go func() {
		data, rerr := exp.readExecOutput(runCtx, stdin, targetPass)
		werr := session.Wait()
		if werr == nil && rerr != nil {
			werr = rerr
		}
		resCh <- execResult{stdout: data, err: werr}
	}()

	select {
	case res := <-resCh:
		out := &domain.CommandOutput{
			Stdout:     res.stdout,
			DurationMs: time.Since(start).Milliseconds(),
		}
		if res.err == nil {
			out.ExitCode = 0
		} else if exitErr, ok := res.err.(*gossh.ExitError); ok {
			out.ExitCode = exitErr.ExitStatus()
		} else if runCtx.Err() != nil {
			return nil, apierr.SSHTimeoutf("command timed out after %ds", int(timeout.Seconds()))
		} else {
			return nil, apierr.Wrap(apierr.CodeSSHError, 502, "su_ssh command execution failed", res.err)
		}
		return out, nil
	case <-runCtx.Done():
		_ = session.Close()
		return nil, apierr.SSHTimeoutf("command timed out after %ds", int(timeout.Seconds()))
	}
}

type sessionWriteCloser struct {
	stdin     io.WriteCloser
	session   *gossh.Session
	closeOnce sync.Once
}

func (s *sessionWriteCloser) Write(p []byte) (int, error) {
	return s.stdin.Write(p)
}

func (s *sessionWriteCloser) Close() error {
	var err error
	s.closeOnce.Do(func() {
		_ = s.stdin.Close()
		err = s.session.Close()
	})
	return err
}

func (p *Pool) openSuSFTP(asset *domain.ServerAsset, all map[string]*domain.ServerAsset) (*sftp.Client, error) {
	ctx, cancel := context.WithTimeout(context.Background(), connectTimeout+suStepTimeout)
	defer cancel()

	session, stdin, exp, err := p.bootstrapSuShell(ctx, asset, all, 80, 24)
	if err != nil {
		return nil, err
	}

	targetPass := p.resolveTargetPassword(asset)
	extraArgs := []string{"-s"}
	if targetPass == "" {
		extraArgs = append(extraArgs, "-o", "BatchMode=yes")
	} else {
		extraArgs = append(extraArgs, "-o", "NumberOfPasswordPrompts=1")
	}

	const rawMarker = "__SSHIELD_RAW_1____SSHIELD_RAW_2__"
	sshCmd := "stty raw -echo; echo __SSHIELD_RAW_1__\"\"__SSHIELD_RAW_2__; exec " + buildTargetSSHCommand(asset, extraArgs, "sftp") + "\n"
	if _, err := io.WriteString(stdin, sshCmd); err != nil {
		_ = session.Close()
		return nil, apierr.Wrap(apierr.CodeSSHError, 502, "launch target ssh sftp", err)
	}
	if err := exp.waitForMarker(ctx, rawMarker, suBootTimeout); err != nil {
		_ = session.Close()
		return nil, apierr.Wrap(apierr.CodeSSHError, 502, "enter raw mode for SFTP", err)
	}

	if targetPass != "" {
		if err := exp.maybeInjectTargetPassword(ctx, stdin, targetPass, true); err != nil {
			_ = session.Close()
			return nil, apierr.Wrap(apierr.CodeSSHError, 502, "target sftp password injection", err)
		}
	}

	sc, err := sftp.NewClientPipe(exp, &sessionWriteCloser{stdin: stdin, session: session})
	if err != nil {
		_ = session.Close()
		return nil, apierr.Wrap(apierr.CodeSSHError, 502, "open su_ssh SFTP subsystem failed", err)
	}
	return sc, nil
}
