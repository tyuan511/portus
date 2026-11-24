// Package security implements the layered command security engine:
//
//	L1 — shell AST evaluation (mvdan/sh) against configurable rules, fail-closed;
//	masking — secret redaction of command output before it reaches agents/audits;
//	L2 — pluggable AI semantic review (OpenAI/Anthropic/SystemOne), fail-closed.
package security

import (
	"bytes"
	"fmt"
	"regexp"
	"sort"
	"strings"

	"mvdan.cc/sh/v3/syntax"

	"github.com/tyuan511/bulwark/internal/domain"
)

// Engine evaluates shell commands against security rules using the AST.
type Engine struct {
	parser  *syntax.Parser
	printer *syntax.Printer
}

// NewEngine builds a bash-variant parser engine.
func NewEngine() *Engine {
	return &Engine{
		parser:  syntax.NewParser(syntax.Variant(syntax.LangBash)),
		printer: syntax.NewPrinter(),
	}
}

// extractedCommand is a single executable invocation found in the AST.
type extractedCommand struct {
	program          string
	args             []string
	raw              string
	context          string
	hasWriteRedirect bool
}

// Evaluate parses command into a shell AST and evaluates every extracted
// invocation against the rules. Fail-closed: parse failures or empty ASTs deny.
func (e *Engine) Evaluate(command string, rules []*domain.SecurityRule) *domain.CommandVerdict {
	trimmed := strings.TrimSpace(command)
	if trimmed == "" {
		return denyVerdict("Empty command is denied (fail-closed)")
	}

	file, err := e.parser.Parse(strings.NewReader(trimmed), "")
	if err != nil {
		return denyVerdict(fmt.Sprintf("Shell AST parse failed (fail-closed): %v", err))
	}

	var extracted []extractedCommand
	for _, stmt := range file.Stmts {
		e.walkStmt(stmt, "root", &extracted)
	}

	if len(extracted) == 0 {
		return &domain.CommandVerdict{
			Decision:       domain.DecisionAskApproval,
			ParseOK:        true,
			ParsedCommands: []domain.ParsedCommandInfo{},
			MatchedRules:   []string{},
			Reason:         "No executable simple commands found; requiring human approval",
		}
	}

	overall := domain.DecisionAllow
	var matchedRules []string
	parsed := make([]domain.ParsedCommandInfo, 0, len(extracted))

	for _, item := range extracted {
		decision, matched := evaluateSingle(item.program, item.args, item.raw, trimmed, rules)

		// Write redirections cannot be auto-allowed even when the base program
		// is whitelisted (`cat foo > /etc/passwd` must not sail through).
		if item.hasWriteRedirect && decision == domain.DecisionAllow {
			decision = domain.DecisionAskApproval
		}

		overall = domain.MoreRestrictive(overall, decision)

		if matched != "" && !containsStr(matchedRules, matched) {
			matchedRules = append(matchedRules, matched)
		}

		parsed = append(parsed, domain.ParsedCommandInfo{
			Program:     item.program,
			Args:        item.args,
			Raw:         item.raw,
			Context:     item.context,
			Decision:    decision,
			MatchedRule: matched,
		})
	}

	var reason string
	switch overall {
	case domain.DecisionAllow:
		reason = fmt.Sprintf("All %d parsed command(s) matched allow rules", len(parsed))
	case domain.DecisionAskApproval:
		reason = "One or more commands require human-in-the-loop approval"
	case domain.DecisionDeny:
		reason = "One or more commands matched a deny rule (blacklist)"
	}

	return &domain.CommandVerdict{
		Decision:       overall,
		ParseOK:        true,
		ParsedCommands: parsed,
		MatchedRules:   matchedRules,
		Reason:         reason,
	}
}

func denyVerdict(reason string) *domain.CommandVerdict {
	return &domain.CommandVerdict{
		Decision:       domain.DecisionDeny,
		ParseOK:        false,
		ParsedCommands: []domain.ParsedCommandInfo{},
		MatchedRules:   []string{},
		Reason:         reason,
	}
}

func containsStr(list []string, s string) bool {
	for _, v := range list {
		if v == s {
			return true
		}
	}
	return false
}

// --- AST walking ---

func (e *Engine) walkStmt(s *syntax.Stmt, ctx string, out *[]extractedCommand) {
	if s == nil {
		return
	}
	writeRedir := stmtHasWriteRedirect(s)

	switch cmd := s.Cmd.(type) {
	case *syntax.CallExpr:
		e.walkCall(cmd, ctx, writeRedir, out)
	case *syntax.BinaryCmd:
		e.walkStmt(cmd.X, ctx, out)
		e.walkStmt(cmd.Y, ctx, out)
	case *syntax.Block:
		for _, st := range cmd.Stmts {
			e.walkStmt(st, ctx+" -> brace", out)
		}
	case *syntax.Subshell:
		for _, st := range cmd.Stmts {
			e.walkStmt(st, ctx+" -> subshell", out)
		}
	case *syntax.IfClause:
		e.walkIf(cmd, ctx, out)
	case *syntax.ForClause:
		for _, st := range cmd.Do {
			e.walkStmt(st, ctx+" -> for", out)
		}
	case *syntax.WhileClause:
		label := "while"
		if cmd.Until {
			label = "until"
		}
		for _, st := range cmd.Cond {
			e.walkStmt(st, ctx+" -> "+label+"_cond", out)
		}
		for _, st := range cmd.Do {
			e.walkStmt(st, ctx+" -> "+label+"_body", out)
		}
	case *syntax.CaseClause:
		for _, item := range cmd.Items {
			for _, st := range item.Stmts {
				e.walkStmt(st, ctx+" -> case", out)
			}
		}
	case *syntax.FuncDecl:
		e.walkStmt(cmd.Body, "function_body", out)
	case *syntax.CoprocClause:
		e.walkStmt(cmd.Stmt, ctx+" -> coproc", out)
	case *syntax.DeclClause:
		for _, as := range cmd.Args {
			e.walkWord(as.Value, ctx, out)
		}
	case *syntax.ArithmCmd, *syntax.TestClause:
		// No direct executable commands; expansions inside are covered elsewhere.
	case nil:
		// Redirect-only statement (e.g. `> file`); nothing executable.
	}
}

func (e *Engine) walkIf(ic *syntax.IfClause, ctx string, out *[]extractedCommand) {
	if ic == nil {
		return
	}
	for _, st := range ic.Cond {
		e.walkStmt(st, ctx+" -> if_cond", out)
	}
	for _, st := range ic.Then {
		e.walkStmt(st, ctx+" -> then", out)
	}
	if ic.Else != nil {
		e.walkIf(ic.Else, ctx+" -> else", out)
	}
}

func (e *Engine) walkCall(call *syntax.CallExpr, ctx string, writeRedir bool, out *[]extractedCommand) {
	// Command substitutions inside assignment values are still commands.
	for _, as := range call.Assigns {
		e.walkWord(as.Value, ctx+" -> assign", out)
	}
	if len(call.Args) == 0 {
		return
	}

	program := e.wordText(call.Args[0])
	args := make([]string, 0, len(call.Args)-1)
	for _, a := range call.Args[1:] {
		args = append(args, e.wordText(a))
	}
	prog, cleanArgs := unwrapWrapper(program, args)

	// Recurse into expansions inside the call arguments (`ls $(rm -rf /)`).
	for _, a := range call.Args {
		e.walkWord(a, ctx, out)
	}

	if prog == "" {
		return
	}
	*out = append(*out, extractedCommand{
		program:          prog,
		args:             cleanArgs,
		raw:              reconstruct(call.Args),
		context:          ctx,
		hasWriteRedirect: writeRedir,
	})
}

// walkWord recurses into command/process substitutions and parameter
// expansions hiding nested commands.
func (e *Engine) walkWord(w *syntax.Word, ctx string, out *[]extractedCommand) {
	if w == nil {
		return
	}
	for _, part := range w.Parts {
		switch p := part.(type) {
		case *syntax.CmdSubst:
			for _, st := range p.Stmts {
				e.walkStmt(st, ctx+" -> cmdsubst", out)
			}
		case *syntax.ProcSubst:
			for _, st := range p.Stmts {
				e.walkStmt(st, ctx+" -> procsubst", out)
			}
		case *syntax.ParamExp:
			e.walkParamExp(p, ctx, out)
		case *syntax.BraceExp:
			for _, el := range p.Elems {
				e.walkWord(el, ctx, out)
			}
		}
	}
}

func (e *Engine) walkParamExp(p *syntax.ParamExp, ctx string, out *[]extractedCommand) {
	if p == nil {
		return
	}
	if p.Repl != nil {
		e.walkWord(p.Repl.Orig, ctx, out)
		e.walkWord(p.Repl.With, ctx, out)
	}
	if p.Exp != nil {
		e.walkWord(p.Exp.Word, ctx, out)
	}
	// Zsh nested ${$(cmd)} style expansions.
	switch np := p.NestedParam.(type) {
	case *syntax.CmdSubst:
		for _, st := range np.Stmts {
			e.walkStmt(st, ctx+" -> nestedparam", out)
		}
	case *syntax.ParamExp:
		e.walkParamExp(np, ctx, out)
	}
	// Index expressions may hide command substitutions: ${a[$(cmd)]}.
	if p.Index != nil {
		e.walkArithm(p.Index, ctx, out)
	}
}

// walkArithm recurses arithmetic expressions for nested command substitutions.
func (e *Engine) walkArithm(a syntax.ArithmExpr, ctx string, out *[]extractedCommand) {
	switch x := a.(type) {
	case *syntax.Word:
		e.walkWord(x, ctx, out)
	case *syntax.BinaryArithm:
		e.walkArithm(x.X, ctx, out)
		e.walkArithm(x.Y, ctx, out)
	case *syntax.UnaryArithm:
		e.walkArithm(x.X, ctx, out)
	case *syntax.ParenArithm:
		e.walkArithm(x.X, ctx, out)
	}
}

func stmtHasWriteRedirect(s *syntax.Stmt) bool {
	for _, r := range s.Redirs {
		switch r.Op {
		case syntax.RdrOut, syntax.AppOut, syntax.RdrInOut, syntax.RdrAll,
			syntax.RdrAllClob, syntax.AppAll, syntax.AppAllClob, syntax.RdrClob:
			return true
		}
	}
	return false
}

// wordText renders a word to text: literal parts directly, anything dynamic
// re-printed from the AST.
func (e *Engine) wordText(w *syntax.Word) string {
	if w == nil {
		return ""
	}
	var sb strings.Builder
	for _, part := range w.Parts {
		if lit, ok := part.(*syntax.Lit); ok {
			sb.WriteString(lit.Value)
			continue
		}
		var buf bytes.Buffer
		if err := e.printer.Print(&buf, part); err == nil {
			sb.WriteString(buf.String())
		}
	}
	return sb.String()
}

func reconstruct(args []*syntax.Word) string {
	parts := make([]string, len(args))
	for i, a := range args {
		var buf bytes.Buffer
		p := syntax.NewPrinter()
		if err := p.Print(&buf, a); err != nil {
			parts[i] = "?"
		} else {
			parts[i] = buf.String()
		}
	}
	return strings.Join(parts, " ")
}

// unwrapWrapper strips privilege/execution wrappers (sudo, doas, nohup,
// command, time, env) so the real command is evaluated against the rules.
func unwrapWrapper(program string, args []string) (string, []string) {
	for {
		switch program {
		case "sudo", "doas":
			i := 0
			for i < len(args) {
				a := args[i]
				if a == "--" {
					i++
					break
				}
				if strings.HasPrefix(a, "-") {
					i++
					// Options taking a separate value (-u root, -g wheel).
					if (a == "-u" || a == "-g" || a == "-C") && i < len(args) {
						i++
					}
					continue
				}
				break
			}
			if i >= len(args) {
				return program, args // wrapper with no inner command; left for rule matching
			}
			program, args = args[i], args[i+1:]
		case "nohup", "command", "time":
			if len(args) == 0 {
				return program, args
			}
			program, args = args[0], args[1:]
		case "env":
			i := 0
			for i < len(args) {
				a := args[i]
				if a == "--" {
					i++
					break
				}
				if strings.HasPrefix(a, "-") || strings.Contains(a, "=") {
					i++
					continue
				}
				break
			}
			if i >= len(args) {
				return program, args
			}
			program, args = args[i], args[i+1:]
		default:
			return program, args
		}
	}
}

// --- rule matching ---

func evaluateSingle(program string, args []string, raw string, fullCommand string, rules []*domain.SecurityRule) (domain.PolicyDecision, string) {
	// Strip path prefix: `/bin/rm` behaves like `rm`.
	base := program
	if idx := strings.LastIndex(base, "/"); idx >= 0 {
		base = base[idx+1:]
	}
	normalized := base
	if len(args) > 0 {
		normalized = base + " " + strings.Join(args, " ")
	}
	rawTrim := strings.TrimSpace(raw)

	active := make([]*domain.SecurityRule, 0, len(rules))
	for _, r := range rules {
		if r.Enabled {
			active = append(active, r)
		}
	}
	sort.SliceStable(active, func(i, j int) bool {
		if active[i].Priority != active[j].Priority {
			return active[i].Priority > active[j].Priority
		}
		// Tie: stricter decision first (deny > ask > allow).
		return domain.MoreRestrictive(active[i].Decision, active[j].Decision) == active[i].Decision &&
			active[i].Decision != active[j].Decision
	})

	for _, rule := range active {
		pattern := strings.TrimSpace(rule.Pattern)
		switch rule.MatchKind {
		case domain.MatchProgramExact:
			if base == pattern {
				return rule.Decision, rule.Name
			}
		case domain.MatchCommandPrefix:
			if normalized == pattern ||
				strings.HasPrefix(normalized, pattern+" ") ||
				rawTrim == pattern ||
				strings.HasPrefix(rawTrim, pattern+" ") {
				return rule.Decision, rule.Name
			}
		case domain.MatchRegex:
			re, err := regexp.Compile(pattern)
			if err == nil && (re.MatchString(rawTrim) || re.MatchString(normalized) || re.MatchString(fullCommand)) {
				return rule.Decision, rule.Name
			}
		}
	}

	// Unknown commands default to human approval.
	return domain.DecisionAskApproval, ""
}
