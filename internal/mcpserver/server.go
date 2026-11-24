// Package mcpserver implements the Bulwark MCP endpoint on the official Go
// SDK: 14 read/execute tools, per-request identity resolved from the bearer
// PAT, full RBAC enforcement, and tamper-evident auditing.
package mcpserver

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"strings"
	"time"

	"github.com/modelcontextprotocol/go-sdk/mcp"
	"go.uber.org/zap"

	"github.com/tyuan511/bulwark/internal/apierr"
	"github.com/tyuan511/bulwark/internal/approvals"
	"github.com/tyuan511/bulwark/internal/domain"
	"github.com/tyuan511/bulwark/internal/notify"
	"github.com/tyuan511/bulwark/internal/rbac"
	"github.com/tyuan511/bulwark/internal/security"
	"github.com/tyuan511/bulwark/internal/sshpool"
	"github.com/tyuan511/bulwark/internal/storage"
)

// IdentityResolver validates a bearer PAT into an identity (auth.Service).
type IdentityResolver interface {
	ResolveApiToken(token string) (*domain.Identity, *domain.ApiToken, error)
}

// Deps wires every subsystem the MCP tools need.
type Deps struct {
	DB     *storage.DB
	Auth   IdentityResolver
	RBAC   *rbac.Service
	Pool   *sshpool.Pool
	Engine *security.Engine
	Broker *approvals.Broker
	Rules  func() []*domain.SecurityRule
	L2     func() security.Reviewer
	Audit  func(ev *domain.AuditEvent)
	Log    *zap.Logger
	// Notify pushes approval/policy events to the configured webhook (may be nil).
	Notify *notify.Notifier

	// SessionTTL bounds each server-issued host session.
	SessionTTL time.Duration
}

const serverVersion = "1.0.0"

// NewHandler builds the streamable-HTTP MCP handler mounted at /mcp.
// Bearer authentication happens in the surrounding middleware; tool handlers
// re-resolve identity from request headers (RequestExtra.Header).
func NewHandler(d Deps) http.Handler {
	srv := mcp.NewServer(&mcp.Implementation{Name: "bulwark", Version: serverVersion}, nil)

	type registration struct {
		name        string
		description string
		schema      string
		handler     toolFunc
	}
	registrations := []registration{
		{"open_host_session", "Request temporary authorization to operate on one host. The server validates access and returns a session_id that execute_command requires; every operation in the session is recorded and replayable.", `{"type":"object","properties":{"server_id":{"type":"string"},"purpose":{"type":"string","description":"what this session is for, recorded in the audit trail"}},"required":["server_id"]}`, tOpenHostSession},
		{"close_host_session", "Close a host session early; further execute_command calls with it will be rejected.", `{"type":"object","properties":{"session_id":{"type":"string"}},"required":["session_id"]}`, tCloseHostSession},
		{"list_authorized_servers", "List SSH servers the caller is authorized to use", `{"type":"object","properties":{}}`, tListAuthorizedServers},
		{"get_server_details", "Get details for one authorized server (no secrets)", `{"type":"object","properties":{"server_id":{"type":"string"}},"required":["server_id"]}`, tGetServerDetails},
		{"test_server_connection", "Test SSH connectivity and authentication to a server", `{"type":"object","properties":{"server_id":{"type":"string"}},"required":["server_id"]}`, tTestServerConnection},
		{"list_security_rules", "List the L1 security rules used for command evaluation", `{"type":"object","properties":{}}`, tListSecurityRules},
		{"evaluate_command", "Evaluate a command against L1 rules and the L2 reviewer without executing", `{"type":"object","properties":{"command":{"type":"string"},"intent":{"type":"string"},"server_id":{"type":"string"}},"required":["command"]}`, tEvaluateCommand},
		{"list_snippets", "List saved command snippets", `{"type":"object","properties":{}}`, tListSnippets},
		{"query_audit_logs", "Query command execution audit logs (scoped to the caller unless admin)", `{"type":"object","properties":{"q":{"type":"string"},"server_id":{"type":"string"},"actor":{"type":"string"},"decision":{"type":"string"},"limit":{"type":"integer"},"offset":{"type":"integer"}}}`, tQueryAuditLogs},
		{"get_audit_stats", "Aggregated audit statistics (scoped to the caller unless admin)", `{"type":"object","properties":{}}`, tGetAuditStats},
		{"verify_audit_chain", "Verify the tamper-evident audit hash chain (admin)", `{"type":"object","properties":{}}`, tVerifyAuditChain},
		{"list_pending_approvals", "List pending command approvals (approvals.decide permission)", `{"type":"object","properties":{}}`, tListPendingApprovals},
		{"get_l2_config", "Get the L2 AI semantic review configuration (rules.read permission)", `{"type":"object","properties":{}}`, tGetL2Config},
		{"execute_command", "Execute a shell command on the host bound to a live session, through the layered security pipeline (L1 → L2 → approval → SSH). Open a session first with open_host_session.", `{"type":"object","properties":{"session_id":{"type":"string","description":"session from open_host_session authorizing this host"},"command":{"type":"string"},"intent":{"type":"string"},"dry_run":{"type":"boolean"},"timeout_secs":{"type":"integer"}},"required":["session_id","command"]}`, tExecuteCommand},
	}

	for _, reg := range registrations {
		reg := reg
		srv.AddTool(&mcp.Tool{
			Name:        reg.name,
			Description: reg.description,
			InputSchema: json.RawMessage(reg.schema),
		}, func(ctx context.Context, req *mcp.CallToolRequest) (*mcp.CallToolResult, error) {
			return d.dispatch(ctx, req, reg.handler)
		})
	}

	return mcp.NewStreamableHTTPHandler(func(r *http.Request) *mcp.Server { return srv },
		&mcp.StreamableHTTPOptions{
			// Stateless + JSON: every request re-authenticates from its own
			// Authorization header (no sticky sessions to hijack), and the
			// surrounding fiber/fasthttp stack lacks http.Flusher so SSE
			// streaming is off; responses use the spec's JSON mode.
			Stateless:    true,
			JSONResponse: true,
		})
}

// toolFunc is the internal handler signature after envelope handling.
type toolFunc func(ctx context.Context, d *Deps, id *domain.Identity, args map[string]any) (any, *apierr.E)

// dispatch resolves the caller identity and wraps results into CallToolResult.
func (d *Deps) dispatch(ctx context.Context, req *mcp.CallToolRequest, fn toolFunc) (*mcp.CallToolResult, error) {
	identity, err := d.identityFromRequest(req)
	if err != nil {
		return nil, err
	}

	var args map[string]any
	if req.Params.Arguments != nil {
		if err := json.Unmarshal(req.Params.Arguments, &args); err != nil {
			return nil, fmt.Errorf("invalid tool arguments: %w", err)
		}
	}
	if args == nil {
		args = map[string]any{}
	}

	payload, apiErr := fn(ctx, d, identity, args)
	if apiErr != nil {
		// Tool-level failures surface as error results (not protocol errors)
		// so the LLM client sees a structured message.
		return errorResult(fmt.Sprintf("[%d] %s", apiErr.Code, apiErr.Message)), nil
	}

	raw, err := json.Marshal(payload)
	if err != nil {
		return nil, err
	}
	return &mcp.CallToolResult{
		Content: []mcp.Content{&mcp.TextContent{Text: string(raw)}},
	}, nil
}

func (d *Deps) identityFromRequest(req *mcp.CallToolRequest) (*domain.Identity, error) {
	var token string
	if req.Extra != nil && req.Extra.Header != nil {
		token = bearerFromHeader(req.Extra.Header.Get("Authorization"))
	}
	if token == "" {
		return nil, fmt.Errorf("missing bearer token")
	}
	identity, _, err := d.Auth.ResolveApiToken(token)
	if err != nil {
		return nil, err
	}
	return identity, nil
}

func bearerFromHeader(header string) string {
	parts := strings.SplitN(header, " ", 2)
	if len(parts) == 2 && strings.EqualFold(parts[0], "Bearer") {
		return strings.TrimSpace(parts[1])
	}
	return ""
}

func errorResult(message string) *mcp.CallToolResult {
	return &mcp.CallToolResult{
		IsError: true,
		Content: []mcp.Content{&mcp.TextContent{Text: message}},
	}
}

// arg helpers

func argString(args map[string]any, key string) string {
	if v, ok := args[key]; ok {
		if s, ok := v.(string); ok {
			return s
		}
	}
	return ""
}

func argBool(args map[string]any, key string) bool {
	if v, ok := args[key]; ok {
		if b, ok := v.(bool); ok {
			return b
		}
	}
	return false
}

func argInt(args map[string]any, key string) (int64, bool) {
	if v, ok := args[key]; ok {
		switch n := v.(type) {
		case float64:
			return int64(n), true
		case json.Number:
			i, err := n.Int64()
			return i, err == nil
		}
	}
	return 0, false
}

// now is stubbed for tests.
var now = func() time.Time { return time.Now().UTC() }
