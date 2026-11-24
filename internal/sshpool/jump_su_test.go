package sshpool

import (
	"bufio"
	"context"
	"io"
	"strings"
	"testing"
	"time"

	"github.com/tyuan511/bulwark/internal/domain"
)

func TestBuildTargetSSHCommand(t *testing.T) {
	srv := &domain.ServerAsset{
		Host:     "10.0.1.25",
		Port:     2222,
		Username: "deploy",
	}

	gotTerm := buildTargetSSHCommand(srv, []string{"-tt"}, "")
	wantTerm := "'ssh' '-o' 'StrictHostKeyChecking=no' '-o' 'ConnectTimeout=10' '-p' '2222' '-tt' 'deploy@10.0.1.25'"
	if gotTerm != wantTerm {
		t.Fatalf("terminal cmd:\ngot:  %s\nwant: %s", gotTerm, wantTerm)
	}

	gotExec := buildTargetSSHCommand(srv, []string{"-T"}, "echo 'hello world'")
	wantExec := "'ssh' '-o' 'StrictHostKeyChecking=no' '-o' 'ConnectTimeout=10' '-p' '2222' '-T' 'deploy@10.0.1.25' 'echo '\\''hello world'\\'''"
	if gotExec != wantExec {
		t.Fatalf("exec cmd:\ngot:  %s\nwant: %s", gotExec, wantExec)
	}

	gotSFTP := buildTargetSSHCommand(srv, []string{"-s"}, "sftp")
	wantSFTP := "'ssh' '-o' 'StrictHostKeyChecking=no' '-o' 'ConnectTimeout=10' '-p' '2222' '-s' 'deploy@10.0.1.25' 'sftp'"
	if gotSFTP != wantSFTP {
		t.Fatalf("sftp cmd:\ngot:  %s\nwant: %s", gotSFTP, wantSFTP)
	}
}

func TestPromptAndFailureMatchers(t *testing.T) {
	if !isPasswordPrompt("Last login: Mon\r\nPassword: ") {
		t.Fatal("expected English Password: prompt to match")
	}
	if !isPasswordPrompt("密码：") {
		t.Fatal("expected Chinese 密码： prompt to match")
	}
	if isPasswordPrompt("[ops@bastion ~]$ ") {
		t.Fatal("shell prompt must not match password prompt")
	}

	if !isAuthFailure("su: Authentication failure\r\n") {
		t.Fatal("expected authentication failure to match")
	}
	if !isAuthFailure("su: Sorry\r\n") {
		t.Fatal("expected su: Sorry to match")
	}

	if !looksLikePrompt("[root@bastion ~]# ") || !looksLikePrompt("ops@jump:~$ ") {
		t.Fatal("expected standard shell prompts to match")
	}
	if !looksLikePrompt("\x1b[32mops@jump\x1b[0m:~$ \x1b[?2004h") {
		t.Fatal("expected ANSI-colored + bracketed-paste prompt to match")
	}
	if !isPasswordPrompt("\x1b[1mdeploy@10.0.1.25's password:\x1b[0m ") {
		t.Fatal("expected ANSI-styled password prompt to match")
	}
}

func TestPtyExpecterWaitForMarkerPreservesTrailingBytes(t *testing.T) {
	pr, pw := io.Pipe()
	exp := newPtyExpecter(pr)

	go func() {
		_, _ = io.WriteString(pw, "Welcome banner\r\n__MARKER__\r\nPAYLOAD")
		_ = pw.Close()
	}()

	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()

	if err := exp.waitForMarker(ctx, "__MARKER__", 2*time.Second); err != nil {
		t.Fatalf("waitForMarker failed: %v", err)
	}
	rest, err := io.ReadAll(exp)
	if err != nil {
		t.Fatalf("ReadAll failed: %v", err)
	}
	if string(rest) != "PAYLOAD" {
		t.Fatalf("trailing bytes = %q, want %q", string(rest), "PAYLOAD")
	}
}

func TestPtyExpecterRunSuExpectSuccess(t *testing.T) {
	outR, outW := io.Pipe()
	inR, inW := io.Pipe()
	exp := newPtyExpecter(outR)

	// Simulate a leftover jump host prompt already buffered right before runSuExpect.
	exp.buf = []byte("[jumpuser@bastion ~]$ ")

	go func() {
		defer outW.Close()
		br := bufio.NewReader(inR)

		line, err := br.ReadString('\n')
		if err != nil || !strings.Contains(line, "exec su - 'ops'") {
			return
		}
		// Jump host echoes the command and outputs the pre-su marker, then execs su.
		_, _ = io.WriteString(outW, line+"\r\n__SSHIELD_PRE_SU_1____SSHIELD_PRE_SU_2__\r\n")
		_, _ = io.WriteString(outW, "Password: ")

		pass, err := br.ReadString('\n')
		if err != nil || strings.TrimSpace(pass) != "s3cr3t" {
			_, _ = io.WriteString(outW, "su: Authentication failure\r\n")
			return
		}
		_, _ = io.WriteString(outW, "\r\n\x1b[32m[ops@bastion ~]$ \x1b[0m\x1b[?2004h")

		probe, err := br.ReadString('\n')
		if err != nil || !strings.Contains(probe, "__SSHIELD_SU_1__") {
			return
		}
		_, _ = io.WriteString(outW, "__SSHIELD_SU_1____SSHIELD_SU_2__\r\n")
	}()

	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()

	if err := exp.runSuExpect(ctx, inW, "ops", "s3cr3t"); err != nil {
		t.Fatalf("runSuExpect failed: %v", err)
	}
}

func TestPtyExpecterRunSuExpectAuthFailure(t *testing.T) {
	outR, outW := io.Pipe()
	inR, inW := io.Pipe()
	exp := newPtyExpecter(outR)

	go func() {
		defer outW.Close()
		br := bufio.NewReader(inR)
		_, _ = br.ReadString('\n')
		_, _ = io.WriteString(outW, "__SSHIELD_PRE_SU_1____SSHIELD_PRE_SU_2__\r\n")
		_, _ = io.WriteString(outW, "Password: ")
		_, _ = br.ReadString('\n')
		_, _ = io.WriteString(outW, "\r\nsu: Authentication failure\r\n")
	}()

	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()

	err := exp.runSuExpect(ctx, inW, "ops", "wrong")
	if err == nil || !strings.Contains(err.Error(), "authentication failed") {
		t.Fatalf("expected authentication failed error, got: %v", err)
	}
}

func TestPtyExpecterMaybeInjectTargetPassword(t *testing.T) {
	outR, outW := io.Pipe()
	inR, inW := io.Pipe()
	exp := newPtyExpecter(outR)

	gotPass := make(chan string, 1)
	go func() {
		defer outW.Close()
		_, _ = io.WriteString(outW, "deploy@10.0.1.25's password: ")
		br := bufio.NewReader(inR)
		line, _ := br.ReadString('\n')
		gotPass <- strings.TrimSpace(line)
	}()

	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()

	if err := exp.maybeInjectTargetPassword(ctx, inW, "target-pw", false); err != nil {
		t.Fatalf("maybeInjectTargetPassword failed: %v", err)
	}
	if p := <-gotPass; p != "target-pw" {
		t.Fatalf("injected password = %q, want %q", p, "target-pw")
	}
}

func TestPtyExpecterMaybeInjectSFTPPasswordDrainsNewline(t *testing.T) {
	outR, outW := io.Pipe()
	inR, inW := io.Pipe()
	exp := newPtyExpecter(outR)

	go func() {
		defer outW.Close()
		_, _ = io.WriteString(outW, "deploy@10.0.1.25's password: ")
		br := bufio.NewReader(inR)
		_, _ = br.ReadString('\n')
		// ssh readpassphrase emits \r\n before the binary SFTP packet starts.
		_, _ = outW.Write([]byte("\r\n\x00\x00\x00\x05\x02"))
	}()

	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()

	if err := exp.maybeInjectTargetPassword(ctx, inW, "target-pw", true); err != nil {
		t.Fatalf("maybeInjectTargetPassword(sftp) failed: %v", err)
	}
	rest, err := io.ReadAll(exp)
	if err != nil {
		t.Fatalf("ReadAll failed: %v", err)
	}
	if string(rest) != "\x00\x00\x00\x05\x02" {
		t.Fatalf("sftp stream = %x, want 0000000502", rest)
	}
}

func TestPtyExpecterReadExecOutputWithPassword(t *testing.T) {
	outR, outW := io.Pipe()
	inR, inW := io.Pipe()
	exp := newPtyExpecter(outR)

	gotPass := make(chan string, 1)
	go func() {
		defer outW.Close()
		_, _ = io.WriteString(outW, "Warning: Permanently added '10.0.1.25' (ED25519) to the list of known hosts.\r\n")
		_, _ = io.WriteString(outW, "deploy@10.0.1.25's ")
		time.Sleep(20 * time.Millisecond)
		_, _ = io.WriteString(outW, "password: ")

		br := bufio.NewReader(inR)
		line, _ := br.ReadString('\n')
		gotPass <- strings.TrimSpace(line)

		// readpassphrase echoes newline, then target command prints output and closes.
		_, _ = io.WriteString(outW, "\nhello from target\n")
	}()

	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()

	out, err := exp.readExecOutput(ctx, inW, "target-pw")
	if err != nil {
		t.Fatalf("readExecOutput failed: %v", err)
	}
	if p := <-gotPass; p != "target-pw" {
		t.Fatalf("injected password = %q, want %q", p, "target-pw")
	}
	if out != "hello from target\n" {
		t.Fatalf("stdout = %q, want %q", out, "hello from target\n")
	}
}

func TestPtyExpecterReadExecOutputNoPromptNeeded(t *testing.T) {
	outR, outW := io.Pipe()
	_, inW := io.Pipe()
	exp := newPtyExpecter(outR)

	go func() {
		defer outW.Close()
		_, _ = io.WriteString(outW, "ok\n")
	}()

	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()

	out, err := exp.readExecOutput(ctx, inW, "unused-pw")
	if err != nil {
		t.Fatalf("readExecOutput failed: %v", err)
	}
	if out != "ok\n" {
		t.Fatalf("stdout = %q, want %q", out, "ok\n")
	}
}
