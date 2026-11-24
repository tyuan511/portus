package sshpool

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"sync"
	"time"

	gossh "golang.org/x/crypto/ssh"

	"github.com/tyuan511/bulwark/internal/apierr"
	"github.com/tyuan511/bulwark/internal/domain"
)

// TerminalInput is a message from the websocket client to the PTY.
type TerminalInput struct {
	// Data carries raw keyboard bytes; nil for resize/close control frames.
	Data []byte
	// Cols/Rows for resize frames (Resize=true).
	Resize bool
	Cols   uint32
	Rows   uint32
	Close  bool
}

// TerminalOutput is a message from the PTY to the websocket client.
type TerminalOutput struct {
	// Data carries PTY bytes; nil when Closed is set.
	Data []byte
	// Closed reports session termination (user typed exit, connection dropped).
	Closed   bool
	ExitCode int
	Reason   string
}

// TerminalSession bridges a websocket-style channel pair to a live PTY.
type TerminalSession struct {
	inputCh   chan TerminalInput
	outputCh  chan TerminalOutput
	closeOnce sync.Once
	done      chan struct{}
}

// Input returns the send channel for client input.
func (t *TerminalSession) Input() chan<- TerminalInput { return t.inputCh }

// Output returns the receive channel for PTY output.
func (t *TerminalSession) Output() <-chan TerminalOutput { return t.outputCh }

// Done is closed when the session has fully terminated.
func (t *TerminalSession) Done() <-chan struct{} { return t.done }

// Close terminates the PTY session idempotently.
func (t *TerminalSession) Close() {
	t.closeOnce.Do(func() { close(t.done) })
}

// OpenTerminal requests a PTY + shell on the target server and returns the
// bridged session. The context governs connection establishment only; use
// Close/TerminalInput.Close to end the session.
func (p *Pool) OpenTerminal(ctx context.Context, asset *domain.ServerAsset, all map[string]*domain.ServerAsset, cols, rows uint32) (*TerminalSession, error) {
	if asset.JumpMode == domain.JumpModeSuSSH {
		return p.openSuTerminal(ctx, asset, all, cols, rows)
	}
	client, err := p.GetOrConnect(asset, all)
	if err != nil {
		return nil, err
	}

	session, err := client.NewSession()
	if err != nil {
		p.Invalidate(asset.ID)
		return nil, apierr.Wrap(apierr.CodeTerminalFailure, 502, "open PTY session", err)
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
		return nil, apierr.Wrap(apierr.CodeTerminalFailure, 502, "request PTY", err)
	}

	stdin, err := session.StdinPipe()
	if err != nil {
		_ = session.Close()
		return nil, apierr.Wrap(apierr.CodeTerminalFailure, 502, "stdin pipe", err)
	}
	stdout, err := session.StdoutPipe()
	if err != nil {
		_ = session.Close()
		return nil, apierr.Wrap(apierr.CodeTerminalFailure, 502, "stdout pipe", err)
	}

	if err := session.Shell(); err != nil {
		_ = session.Close()
		return nil, apierr.Wrap(apierr.CodeTerminalFailure, 502, "start shell", err)
	}

	return bridgeTerminalSession(ctx, session, stdin, stdout), nil
}

func bridgeTerminalSession(ctx context.Context, session *gossh.Session, stdin io.WriteCloser, stdout io.Reader) *TerminalSession {
	ts := &TerminalSession{
		inputCh:  make(chan TerminalInput, 64),
		outputCh: make(chan TerminalOutput, 256),
		done:     make(chan struct{}),
	}

	// Output pump: PTY stdout/stderr → output channel.
	go func() {
		defer ts.Close()
		buf := make([]byte, 16*1024)
		for {
			n, err := stdout.Read(buf)
			if n > 0 {
				chunk := make([]byte, n)
				copy(chunk, buf[:n])
				select {
				case ts.outputCh <- TerminalOutput{Data: chunk}:
				case <-ts.done:
					return
				}
			}
			if err != nil {
				break
			}
		}

		exitCode := -1
		reason := "session closed"
		if werr := session.Wait(); werr != nil {
			if exitErr, ok := werr.(*gossh.ExitError); ok {
				exitCode = exitErr.ExitStatus()
				reason = fmt.Sprintf("shell exited with code %d", exitCode)
			} else if _, ok := werr.(*gossh.ExitMissingError); ok {
				reason = "connection closed by remote host"
			} else {
				reason = "connection lost: " + werr.Error()
			}
		} else {
			exitCode = 0
			reason = "shell exited cleanly"
		}
		select {
		case ts.outputCh <- TerminalOutput{Closed: true, ExitCode: exitCode, Reason: reason}:
		default:
		}
	}()

	// Input pump: input channel → PTY stdin + window-change requests.
	go func() {
		defer func() {
			_ = stdin.Close()
			_ = session.Close()
		}()
		for {
			select {
			case in := <-ts.inputCh:
				if in.Close {
					ts.Close()
					return
				}
				if in.Resize {
					_, _ = session.SendRequest("window-change", true, gossh.Marshal(
						struct{ Cols, Rows, Width, Height uint32 }{in.Cols, in.Rows, 0, 0}))
					continue
				}
				if len(in.Data) > 0 {
					if _, err := stdin.Write(in.Data); err != nil {
						ts.Close()
						return
					}
				}
			case <-ts.done:
				return
			}
		}
	}()

	// Watch the underlying SSH connection: if it dies, end the session.
	go func() {
		select {
		case <-ts.done:
		case <-time.After(24 * time.Hour):
			ts.Close()
		case <-ctx.Done():
			ts.Close()
		}
	}()

	return ts
}

// MarshalInput serializes a JSON terminal control frame into TerminalInput
// ({"type":"input","data":...} / {"type":"resize","cols":..,"rows":..}).
func MarshalInput(raw []byte) (TerminalInput, error) {
	var frame struct {
		Type string `json:"type"`
		Data string `json:"data"`
		Cols uint32 `json:"cols"`
		Rows uint32 `json:"rows"`
	}
	if err := json.Unmarshal(raw, &frame); err != nil {
		return TerminalInput{}, err
	}
	switch frame.Type {
	case "input":
		return TerminalInput{Data: []byte(frame.Data)}, nil
	case "resize":
		return TerminalInput{Resize: true, Cols: frame.Cols, Rows: frame.Rows}, nil
	case "close":
		return TerminalInput{Close: true}, nil
	}
	return TerminalInput{}, fmt.Errorf("unknown terminal frame type %q", frame.Type)
}

// defaultKeyHint is used in error messages when no auth material is available.
func defaultKeyHint() string {
	home, _ := os.UserHomeDir()
	if home == "" {
		return "~/.ssh"
	}
	return home + "/.ssh"
}
