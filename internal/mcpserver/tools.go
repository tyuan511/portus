package mcpserver

import (
	"context"
	"fmt"
	"strings"
	"time"

	"github.com/yuantang/sshield/internal/apierr"
	"github.com/yuantang/sshield/internal/domain"
	"github.com/yuantang/sshield/internal/security"
	"github.com/yuantang/sshield/internal/storage"
)

// --- read tools ---

func tListAuthorizedServers(ctx context.Context, d *Deps, id *domain.Identity, args map[string]any) (any, *apierr.E) {
	servers, err := d.RBAC.ListAccessibleServers(id, false)
	if err != nil {
		return nil, apierr.From(err)
	}
	out := make([]domain.AuthorizedServerInfo, 0, len(servers))
	for _, s := range servers {
		out = append(out, domain.FromAsset(s))
	}
	return map[string]any{"servers": out}, nil
}

func loadAuthorizedServer(d *Deps, id *domain.Identity, serverID string, needExecute bool) (*domain.ServerAsset, *apierr.E) {
	asset, err := d.DB.GetServer(serverID)
	if err == storage.ErrNotFound {
		return nil, apierr.NotFoundf("server %s not found", serverID)
	}
	if err != nil {
		return nil, apierr.StorageErr(err)
	}
	if !asset.Enabled {
		return nil, apierr.Unprocessable("server is disabled")
	}
	if denied := d.RBAC.CheckServerAccess(id, asset, needExecute); denied != nil {
		return nil, apierr.From(denied)
	}
	return asset, nil
}

func tGetServerDetails(ctx context.Context, d *Deps, id *domain.Identity, args map[string]any) (any, *apierr.E) {
	asset, apiErr := loadAuthorizedServer(d, id, argString(args, "server_id"), false)
	if apiErr != nil {
		return nil, apiErr
	}
	return map[string]any{"server": domain.FromAsset(asset)}, nil
}

func tTestServerConnection(ctx context.Context, d *Deps, id *domain.Identity, args map[string]any) (any, *apierr.E) {
	asset, apiErr := loadAuthorizedServer(d, id, argString(args, "server_id"), false)
	if apiErr != nil {
		return nil, apiErr
	}
	all, err := d.DB.ListServers()
	if err != nil {
		return nil, apierr.StorageErr(err)
	}
	if err := d.Pool.TestConnection(asset, assetToMap(all)); err != nil {
		return map[string]any{"ok": false, "message": err.Error()}, nil
	}
	return map[string]any{"ok": true, "message": "SSH connection & auth succeeded"}, nil
}

func tListSecurityRules(ctx context.Context, d *Deps, id *domain.Identity, args map[string]any) (any, *apierr.E) {
	rules, err := d.DB.ListRules()
	if err != nil {
		return nil, apierr.StorageErr(err)
	}
	return map[string]any{"rules": rules}, nil
}

func tEvaluateCommand(ctx context.Context, d *Deps, id *domain.Identity, args map[string]any) (any, *apierr.E) {
	command := argString(args, "command")
	if command == "" {
		return nil, apierr.BadRequest("command is required")
	}
	intent := argString(args, "intent")

	var assetName string
	var assetTags []string
	if serverID := argString(args, "server_id"); serverID != "" {
		asset, apiErr := loadAuthorizedServer(d, id, serverID, false)
		if apiErr != nil {
			return nil, apiErr
		}
		assetName, assetTags = asset.Name, asset.Tags
	} else {
		assetName, assetTags = "sandbox", []string{}
	}

	verdict := d.Engine.Evaluate(command, d.Rules())
	final := verdict.Decision
	var l2 *domain.SemanticReviewVerdict
	if reviewer := d.L2(); reviewer != nil {
		v, err := reviewer.Review(ctx, &domain.SemanticReviewRequest{
			ServerName: assetName, ServerTags: assetTags,
			Command: command, Intent: intent, L1Verdict: *verdict,
		})
		if err != nil {
			return nil, apierr.New(apierr.CodeL2ReviewFailed, 503, err.Error())
		}
		l2 = v
		final = domain.MoreRestrictive(final, v.Decision)
	}
	return map[string]any{
		"l1": verdict, "l2": l2, "final_decision": final,
	}, nil
}

func tListSnippets(ctx context.Context, d *Deps, id *domain.Identity, args map[string]any) (any, *apierr.E) {
	snippets, err := d.DB.ListSnippets()
	if err != nil {
		return nil, apierr.StorageErr(err)
	}
	return map[string]any{"snippets": snippets}, nil
}

// --- audit tools (permission-scoped) ---

func (d *Deps) auditScope(id *domain.Identity, args map[string]any) *domain.AuditQuery {
	q := &domain.AuditQuery{
		Q:        argString(args, "q"),
		ServerID: argString(args, "server_id"),
		Actor:    argString(args, "actor"),
	}
	if dec := argString(args, "decision"); dec != "" {
		q.Decision = domain.PolicyDecision(dec)
	}
	if limit, ok := argInt(args, "limit"); ok {
		q.Limit = int(limit)
	}
	if offset, ok := argInt(args, "offset"); ok {
		q.Offset = int(offset)
	}
	// Members always see only their own trail; admins may query everything.
	if !id.HasPermission(domain.PermAuditViewAll) {
		q.UserID = id.UserID
	}
	return q
}

func tQueryAuditLogs(ctx context.Context, d *Deps, id *domain.Identity, args map[string]any) (any, *apierr.E) {
	if !id.HasPermission(domain.PermAuditViewAll) && !id.HasPermission(domain.PermAuditViewSelf) {
		return nil, apierr.Forbidden("missing audit permission")
	}
	events, err := d.DB.QueryAudits(d.auditScope(id, args))
	if err != nil {
		return nil, apierr.StorageErr(err)
	}
	return map[string]any{"events": events}, nil
}

func tGetAuditStats(ctx context.Context, d *Deps, id *domain.Identity, args map[string]any) (any, *apierr.E) {
	if !id.HasPermission(domain.PermAuditViewAll) && !id.HasPermission(domain.PermAuditViewSelf) {
		return nil, apierr.Forbidden("missing audit permission")
	}
	userID := ""
	if !id.HasPermission(domain.PermAuditViewAll) {
		userID = id.UserID
	}
	stats, err := d.DB.AuditStats(&storage.AuditStatsFilter{UserID: userID})
	if err != nil {
		return nil, apierr.StorageErr(err)
	}
	return stats, nil
}

func tVerifyAuditChain(ctx context.Context, d *Deps, id *domain.Identity, args map[string]any) (any, *apierr.E) {
	if !id.HasPermission(domain.PermAuditViewAll) {
		return nil, apierr.Forbidden("verifying the audit chain requires admin permission")
	}
	status, err := d.DB.VerifyAuditChain()
	if err != nil {
		return nil, apierr.StorageErr(err)
	}
	return status, nil
}

func tListPendingApprovals(ctx context.Context, d *Deps, id *domain.Identity, args map[string]any) (any, *apierr.E) {
	if !id.HasPermission(domain.PermApprovalsDecide) {
		return nil, apierr.Forbidden("missing approvals.decide permission")
	}
	return map[string]any{"approvals": d.Broker.ListPending()}, nil
}

func tGetL2Config(ctx context.Context, d *Deps, id *domain.Identity, args map[string]any) (any, *apierr.E) {
	cfgL2, err := d.DB.GetL2Config()
	if err != nil {
		return nil, apierr.StorageErr(err)
	}
	return cfgL2, nil
}

// --- execute_command: the full layered pipeline ---

type executeArgs struct {
	ServerID   string
	Command    string
	Intent     string
	DryRun     bool
	TimeoutSec int64
}

func parseExecuteArgs(args map[string]any) (executeArgs, *apierr.E) {
	ea := executeArgs{
		ServerID: argString(args, "server_id"),
		Command:  argString(args, "command"),
		Intent:   argString(args, "intent"),
		DryRun:   argBool(args, "dry_run"),
	}
	if ea.ServerID == "" || ea.Command == "" {
		return ea, apierr.BadRequest("server_id and command are required")
	}
	if ea.Intent == "" {
		ea.Intent = "AI-assisted operation"
	}
	if t, ok := argInt(args, "timeout_secs"); ok && t > 0 {
		ea.TimeoutSec = t
	} else {
		ea.TimeoutSec = 120
	}
	return ea, nil
}

// fail records the audit event for a failed execution attempt.
// (kept as a hook so callers audit before returning the structured error)
func tExecuteCommand(ctx context.Context, d *Deps, id *domain.Identity, args map[string]any) (any, *apierr.E) {
	ea, apiErr := parseExecuteArgs(args)
	if apiErr != nil {
		return nil, apiErr
	}

	auditID := storage.GenerateID("aud")
	traceID := storage.GenerateID("mcp")
	fail := func(err error, decision domain.PolicyDecision, approvalID string, approvalStatus domain.ApprovalDecision) (any, *apierr.E) {
		d.auditExecution(d, id, ea, traceID, auditID, decision, nil, nil, approvalID, approvalStatus, nil, nil, "", "", err.Error())
		return nil, apierr.From(err)
	}

	asset, apiErr := loadAuthorizedServer(d, id, ea.ServerID, true)
	if apiErr != nil {
		return nil, apiErr
	}

	// L1: AST rule evaluation (fail-closed).
	verdict := d.Engine.Evaluate(ea.Command, d.Rules())
	if verdict.Decision == domain.DecisionDeny {
		denied := apierr.New(apierr.CodePolicyDenied, 403, "security policy denied command: "+verdict.Reason)
		fail(denied, domain.DecisionDeny, "", "")
		return nil, denied
	}

	// L2: semantic review when enabled.
	var l2 *domain.SemanticReviewVerdict
	if reviewer := d.L2(); reviewer != nil {
		v, err := reviewer.Review(ctx, &domain.SemanticReviewRequest{
			ServerName: asset.Name, ServerTags: asset.Tags,
			Command: ea.Command, Intent: ea.Intent, L1Verdict: *verdict,
		})
		if err != nil {
			return fail(err, domain.DecisionAskApproval, "", "")
		}
		l2 = v
	}

	final := verdict.Decision
	if l2 != nil {
		final = domain.MoreRestrictive(final, l2.Decision)
	}

	if ea.DryRun {
		d.auditExecution(d, id, ea, traceID, auditID, final, verdict, l2, "", "", nil, nil, "", "", "")
		return map[string]any{
			"dry_run":        true,
			"final_decision": final,
			"l1":             verdict,
			"l2":             l2,
		}, nil
	}

	// Approval gate.
	if final == domain.DecisionAskApproval {
		req := domain.ApprovalRequest{
			ID:          storage.GenerateID("appr"),
			TraceID:     traceID,
			CreatedAt:   now(),
			ServerID:    asset.ID,
			ServerName:  asset.Name,
			Host:        asset.Host,
			Username:    asset.Username,
			Command:     ea.Command,
			Intent:      ea.Intent,
			Verdict:     *verdict,
			L2Verdict:   l2,
			TimeoutSecs: 120,
			UserID:      id.UserID,
			RequestedBy: id.Username,
		}
		decision := d.Broker.Request(req, time.Duration(req.TimeoutSecs)*time.Second)
		if decision != domain.ApprovalApproved {
			denied := apierr.New(apierr.CodeApprovalDenied, 403,
				fmt.Sprintf("approval %s for command on %s", decision, asset.Name))
			fail(denied, domain.DecisionAskApproval, req.ID, decision)
			return nil, denied
		}
		auditID = req.ID
		d.auditExecution(d, id, ea, traceID, auditID, domain.DecisionAskApproval, verdict, l2, req.ID, domain.ApprovalApproved, nil, nil, "", "", "")
	}

	// SSH execution with output masking.
	all, err := d.DB.ListServers()
	if err != nil {
		return nil, apierr.StorageErr(err)
	}
	timeout := time.Duration(ea.TimeoutSec) * time.Second
	out, err := d.Pool.ExecuteCommand(ctx, asset, assetToMap(all), ea.Command, timeout)
	if err != nil {
		return fail(err, domain.DecisionAllow, "", "")
	}
	stdout := security.MaskSensitiveOutput(out.Stdout)
	stderr := security.MaskSensitiveOutput(out.Stderr)
	d.auditExecution(d, id, ea, traceID, auditID, domain.DecisionAllow, verdict, l2, "", "",
		&out.ExitCode, &out.DurationMs,
		preview(stdout, 2000), preview(stderr, 2000), "")

	return map[string]any{
		"exit_code":   out.ExitCode,
		"stdout":      stdout,
		"stderr":      stderr,
		"duration_ms": out.DurationMs,
		"server":      asset.Name,
		"audit_id":    auditID,
	}, nil
}

// auditExecution writes one structured event per execution outcome.
func (d *Deps) auditExecution(d2 *Deps, id *domain.Identity, ea executeArgs, traceID, auditID string,
	decision domain.PolicyDecision, l1 *domain.CommandVerdict, l2 *domain.SemanticReviewVerdict,
	approvalID string, approvalStatus domain.ApprovalDecision,
	exitCode *int, durationMs *int64, stdout, stderr, errMsg string) {

	asset, _ := d2.DB.GetServer(ea.ServerID)
	ev := &domain.AuditEvent{
		ID: auditID, TraceID: traceID, Actor: "mcp-http", Timestamp: now(),
		Command: ea.Command, Intent: ea.Intent, Decision: decision,
		UserID: id.UserID, Username: id.Username,
		ApprovalID: approvalID, ApprovalStatus: approvalStatus,
		ExitCode: exitCode, DurationMs: durationMs,
		StdoutPreview: stdout, StderrPreview: stderr, Error: errMsg,
		L1Verdict: l1, L2Verdict: l2,
	}
	if asset != nil {
		ev.ServerID = asset.ID
		ev.ServerName = asset.Name
		ev.SSHUser = asset.Username
		ev.ServerHost = fmt.Sprintf("%s:%d", asset.Host, asset.Port)
	} else {
		ev.ServerID = ea.ServerID
	}
	d2.Audit(ev)
}

func assetToMap(all []*domain.ServerAsset) map[string]*domain.ServerAsset {
	m := make(map[string]*domain.ServerAsset, len(all))
	for _, a := range all {
		m[a.ID] = a
	}
	return m
}

func preview(s string, n int) string {
	if len(s) <= n {
		return strings.TrimSuffix(s, "\n")
	}
	return string([]rune(s)[:n]) + "...[truncated]"
}
