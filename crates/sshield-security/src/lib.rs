//! Dual-layer command security engine:
//! - L1: Shell AST parsing & whitelist/blacklist enforcement (brush-parser)
//! - L2: AI semantic intent verification (Chat/Responses/Messages + SystemOne) & output masking

pub mod l2;
pub mod masking;

use std::io::Cursor;

use brush_parser::ast::{
    AndOr, Command, CommandPrefixOrSuffixItem, CompoundCommand, CompoundList,
    IoFileRedirectKind, IoRedirect, Pipeline, Program, SimpleCommand, Word,
};
use brush_parser::word::{self, WordPiece, WordPieceWithSource};
use brush_parser::{Parser, ParserOptions};
use regex::Regex;
use sshield_core::{CommandVerdict, ParsedCommandInfo, PolicyDecision, RuleMatchKind, SecurityRule};

/// L1 AST-based command security evaluator.
pub struct SecurityEngine {
    parser_options: ParserOptions,
}

impl Default for SecurityEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl SecurityEngine {
    pub fn new() -> Self {
        Self {
            parser_options: ParserOptions::default(),
        }
    }

    /// Built-in default rules for Phase 1 MVP.
    pub fn default_rules() -> Vec<SecurityRule> {
        let allow_programs = [
            "ls", "ll", "pwd", "whoami", "id", "uname", "uptime", "date", "df", "du", "free",
            "top", "htop", "ps", "pgrep", "ss", "netstat", "ip", "ifconfig", "ping", "dig",
            "nslookup", "cat", "head", "tail", "less", "grep", "egrep", "rg", "awk", "sed",
            "wc", "sort", "uniq", "find", "which", "whereis", "echo", "printf", "env", "printenv",
            "journalctl",
        ];

        let allow_prefixes = [
            "docker ps",
            "docker logs",
            "docker inspect",
            "docker stats",
            "docker images",
            "kubectl get",
            "kubectl describe",
            "kubectl logs",
            "systemctl status",
            "git status",
            "git log",
            "git diff",
            "nginx -t",
        ];

        let deny_programs = [
            "rm", "rmdir", "mkfs", "fdisk", "parted", "dd", "shutdown", "reboot", "poweroff",
            "halt", "init", "telinit", "userdel", "groupdel", "passwd", "chpasswd", "visudo",
            "iptables", "nft", "ufw",
        ];

        let deny_prefixes = [
            "chmod -R 777",
            "chown -R",
            "kill -9 1",
            "docker rm",
            "docker rmi",
            "docker system prune",
            "kubectl delete",
        ];

        let mut rules = Vec::new();
        let mut idx = 1;

        for prog in deny_programs {
            rules.push(SecurityRule {
                id: format!("builtin-deny-{idx}"),
                name: format!("Block `{prog}`"),
                match_kind: RuleMatchKind::ProgramExact,
                pattern: prog.to_string(),
                decision: PolicyDecision::Deny,
                description: Some("Built-in destructive command blacklist".to_string()),
                enabled: true,
                priority: 100,
            });
            idx += 1;
        }

        for prefix in deny_prefixes {
            rules.push(SecurityRule {
                id: format!("builtin-deny-{idx}"),
                name: format!("Block `{prefix}`"),
                match_kind: RuleMatchKind::CommandPrefix,
                pattern: prefix.to_string(),
                decision: PolicyDecision::Deny,
                description: Some("Built-in high-risk prefix blacklist".to_string()),
                enabled: true,
                priority: 95,
            });
            idx += 1;
        }

        for prefix in allow_prefixes {
            rules.push(SecurityRule {
                id: format!("builtin-allow-{idx}"),
                name: format!("Allow `{prefix}`"),
                match_kind: RuleMatchKind::CommandPrefix,
                pattern: prefix.to_string(),
                decision: PolicyDecision::Allow,
                description: Some("Built-in read-only diagnostic prefix".to_string()),
                enabled: true,
                priority: 50,
            });
            idx += 1;
        }

        for prog in allow_programs {
            rules.push(SecurityRule {
                id: format!("builtin-allow-{idx}"),
                name: format!("Allow `{prog}`"),
                match_kind: RuleMatchKind::ProgramExact,
                pattern: prog.to_string(),
                decision: PolicyDecision::Allow,
                description: Some("Built-in read-only command whitelist".to_string()),
                enabled: true,
                priority: 10,
            });
            idx += 1;
        }

        rules
    }

    /// Parse `command` into a Shell AST and evaluate all extracted invocations against `rules`.
    /// Follows Fail-Closed principle: any parse failure or empty AST results in `PolicyDecision::Deny`.
    pub fn evaluate(&self, command: &str, rules: &[SecurityRule]) -> CommandVerdict {
        let trimmed = command.trim();
        if trimmed.is_empty() {
            return CommandVerdict {
                decision: PolicyDecision::Deny,
                parse_ok: false,
                parsed_commands: Vec::new(),
                matched_rules: Vec::new(),
                reason: "Empty command is denied (fail-closed)".to_string(),
            };
        }

        let mut parser = Parser::new(Cursor::new(trimmed.as_bytes()), &self.parser_options);
        let program = match parser.parse_program() {
            Ok(prog) => prog,
            Err(err) => {
                return CommandVerdict {
                    decision: PolicyDecision::Deny,
                    parse_ok: false,
                    parsed_commands: Vec::new(),
                    matched_rules: Vec::new(),
                    reason: format!("Shell AST parse failed (fail-closed): {err}"),
                };
            }
        };

        let mut extracted = Vec::new();
        if let Err(err) = self.extract_from_program(&program, "root", &mut extracted) {
            return CommandVerdict {
                decision: PolicyDecision::Deny,
                parse_ok: false,
                parsed_commands: Vec::new(),
                matched_rules: Vec::new(),
                reason: format!("Shell word/subshell parse failed (fail-closed): {err}"),
            };
        }

        if extracted.is_empty() {
            return CommandVerdict {
                decision: PolicyDecision::AskApproval,
                parse_ok: true,
                parsed_commands: Vec::new(),
                matched_rules: Vec::new(),
                reason: "No executable simple commands found; requiring human approval".to_string(),
            };
        }

        let mut overall = PolicyDecision::Allow;
        let mut matched_rules = Vec::new();
        let mut parsed_commands = Vec::with_capacity(extracted.len());

        for item in extracted {
            let (mut decision, matched_rule) = evaluate_single(&item.program, &item.args, &item.raw, rules);

            // File write redirections (e.g. `cat foo > /etc/passwd`) cannot be auto-allowed
            // even if the base program (`cat`, `echo`) is in the read-only whitelist.
            if item.has_write_redirect && decision == PolicyDecision::Allow {
                decision = PolicyDecision::AskApproval;
            }

            if decision > overall {
                overall = decision;
            }

            if let Some(ref rule_name) = matched_rule
                && !matched_rules.contains(rule_name)
            {
                matched_rules.push(rule_name.clone());
            }

            parsed_commands.push(ParsedCommandInfo {
                program: item.program,
                args: item.args,
                raw: item.raw,
                context: item.context,
                decision,
                matched_rule,
            });
        }

        let reason = match overall {
            PolicyDecision::Allow => {
                format!("All {} parsed command(s) matched allow rules", parsed_commands.len())
            }
            PolicyDecision::AskApproval => {
                "One or more commands require human-in-the-loop approval".to_string()
            }
            PolicyDecision::Deny => {
                "One or more commands matched a deny rule (blacklist)".to_string()
            }
        };

        CommandVerdict {
            decision: overall,
            parse_ok: true,
            parsed_commands,
            matched_rules,
            reason,
        }
    }

    fn extract_from_program(
        &self,
        program: &Program,
        context: &str,
        out: &mut Vec<ExtractedCommand>,
    ) -> Result<(), String> {
        for complete_cmd in &program.complete_commands {
            self.extract_from_compound_list(complete_cmd, context, out)?;
        }
        Ok(())
    }

    fn extract_from_compound_list(
        &self,
        list: &CompoundList,
        context: &str,
        out: &mut Vec<ExtractedCommand>,
    ) -> Result<(), String> {
        for item in &list.0 {
            self.extract_from_pipeline(&item.0.first, context, out)?;
            for and_or in &item.0.additional {
                let pipeline = match and_or {
                    AndOr::And(p) | AndOr::Or(p) => p,
                };
                self.extract_from_pipeline(pipeline, context, out)?;
            }
        }
        Ok(())
    }

    fn extract_from_pipeline(
        &self,
        pipeline: &Pipeline,
        context: &str,
        out: &mut Vec<ExtractedCommand>,
    ) -> Result<(), String> {
        let pipe_ctx = if pipeline.seq.len() > 1 {
            format!("{context} -> pipeline")
        } else {
            context.to_string()
        };
        for cmd in &pipeline.seq {
            self.extract_from_command(cmd, &pipe_ctx, out)?;
        }
        Ok(())
    }

    fn extract_from_command(
        &self,
        cmd: &Command,
        context: &str,
        out: &mut Vec<ExtractedCommand>,
    ) -> Result<(), String> {
        match cmd {
            Command::Simple(simple) => self.extract_from_simple(simple, context, out),
            Command::Compound(compound, _redirects) => {
                self.extract_from_compound(compound, context, out)
            }
            Command::Function(func_def) => {
                self.extract_from_compound(&func_def.body.0, "function_body", out)
            }
            Command::ExtendedTest(_, _) => Ok(()),
        }
    }

    fn extract_from_compound(
        &self,
        compound: &CompoundCommand,
        context: &str,
        out: &mut Vec<ExtractedCommand>,
    ) -> Result<(), String> {
        match compound {
            CompoundCommand::BraceGroup(bg) => {
                self.extract_from_compound_list(&bg.list, &format!("{context} -> brace"), out)
            }
            CompoundCommand::Subshell(sub) => {
                self.extract_from_compound_list(&sub.list, &format!("{context} -> subshell"), out)
            }
            CompoundCommand::ForClause(fc) => {
                self.extract_from_compound_list(&fc.body.list, &format!("{context} -> for"), out)
            }
            CompoundCommand::ArithmeticForClause(afc) => {
                self.extract_from_compound_list(&afc.body.list, &format!("{context} -> for"), out)
            }
            CompoundCommand::CaseClause(cc) => {
                for case_item in &cc.cases {
                    if let Some(ref body) = case_item.cmd {
                        self.extract_from_compound_list(body, &format!("{context} -> case"), out)?;
                    }
                }
                Ok(())
            }
            CompoundCommand::IfClause(ic) => {
                self.extract_from_compound_list(&ic.condition, &format!("{context} -> if_cond"), out)?;
                self.extract_from_compound_list(&ic.then, &format!("{context} -> if_then"), out)?;
                if let Some(ref elses) = ic.elses {
                    for else_clause in elses {
                        if let Some(ref cond) = else_clause.condition {
                            self.extract_from_compound_list(
                                cond,
                                &format!("{context} -> elif_cond"),
                                out,
                            )?;
                        }
                        self.extract_from_compound_list(
                            &else_clause.body,
                            &format!("{context} -> else"),
                            out,
                        )?;
                    }
                }
                Ok(())
            }
            CompoundCommand::WhileClause(wc) | CompoundCommand::UntilClause(wc) => {
                self.extract_from_compound_list(&wc.0, &format!("{context} -> loop_cond"), out)?;
                self.extract_from_compound_list(&wc.1.list, &format!("{context} -> loop_body"), out)
            }
            CompoundCommand::Coprocess(coproc) => {
                self.extract_from_command(&coproc.body, &format!("{context} -> coproc"), out)
            }
            CompoundCommand::Arithmetic(_) => Ok(()),
        }
    }

    fn extract_from_simple(
        &self,
        simple: &SimpleCommand,
        context: &str,
        out: &mut Vec<ExtractedCommand>,
    ) -> Result<(), String> {
        let mut words_to_inspect: Vec<&Word> = Vec::new();
        let mut has_write_redirect = false;

        if let Some(ref prefix) = simple.prefix {
            for item in &prefix.0 {
                match item {
                    CommandPrefixOrSuffixItem::Word(w) => words_to_inspect.push(w),
                    CommandPrefixOrSuffixItem::AssignmentWord(_, w) => words_to_inspect.push(w),
                    CommandPrefixOrSuffixItem::IoRedirect(redir) => {
                        if is_write_redirect(redir) {
                            has_write_redirect = true;
                        }
                    }
                    CommandPrefixOrSuffixItem::ProcessSubstitution(_, sub) => {
                        self.extract_from_compound_list(
                            &sub.list,
                            &format!("{context} -> proc_subst"),
                            out,
                        )?;
                    }
                }
            }
        }

        let program = simple
            .word_or_name
            .as_ref()
            .map(|w| w.value.clone())
            .unwrap_or_default();

        if let Some(ref w) = simple.word_or_name {
            words_to_inspect.push(w);
        }

        let mut args = Vec::new();
        if let Some(ref suffix) = simple.suffix {
            for item in &suffix.0 {
                match item {
                    CommandPrefixOrSuffixItem::Word(w) => {
                        args.push(w.value.clone());
                        words_to_inspect.push(w);
                    }
                    CommandPrefixOrSuffixItem::AssignmentWord(_, w) => {
                        args.push(w.value.clone());
                        words_to_inspect.push(w);
                    }
                    CommandPrefixOrSuffixItem::IoRedirect(redir) => {
                        if is_write_redirect(redir) {
                            has_write_redirect = true;
                        }
                    }
                    CommandPrefixOrSuffixItem::ProcessSubstitution(_, sub) => {
                        self.extract_from_compound_list(
                            &sub.list,
                            &format!("{context} -> proc_subst"),
                            out,
                        )?;
                    }
                }
            }
        }

        // Inspect all words for hidden command substitutions `$(...)` or `` `...` ``
        for word in words_to_inspect {
            self.extract_from_word(word, context, out)?;
        }

        if !program.is_empty() {
            out.push(ExtractedCommand {
                program,
                args,
                raw: simple.to_string(),
                context: context.to_string(),
                has_write_redirect,
            });
        }

        Ok(())
    }

    fn extract_from_word(
        &self,
        word: &Word,
        context: &str,
        out: &mut Vec<ExtractedCommand>,
    ) -> Result<(), String> {
        let pieces = word::parse(&word.value, &self.parser_options)
            .map_err(|e| format!("word `{}`: {e}", word.value))?;
        self.extract_from_word_pieces(&pieces, context, out)
    }

    fn extract_from_word_pieces(
        &self,
        pieces: &[WordPieceWithSource],
        context: &str,
        out: &mut Vec<ExtractedCommand>,
    ) -> Result<(), String> {
        for piece in pieces {
            match &piece.piece {
                WordPiece::CommandSubstitution(inner)
                | WordPiece::BackquotedCommandSubstitution(inner) => {
                    let mut sub_parser =
                        Parser::new(Cursor::new(inner.as_bytes()), &self.parser_options);
                    let sub_prog = sub_parser
                        .parse_program()
                        .map_err(|e| format!("command substitution `{inner}`: {e}"))?;
                    self.extract_from_program(
                        &sub_prog,
                        &format!("{context} -> cmd_subst"),
                        out,
                    )?;
                }
                WordPiece::DoubleQuotedSequence(inner_pieces)
                | WordPiece::GettextDoubleQuotedSequence(inner_pieces) => {
                    self.extract_from_word_pieces(inner_pieces, context, out)?;
                }
                _ => {}
            }
        }
        Ok(())
    }
}

struct ExtractedCommand {
    program: String,
    args: Vec<String>,
    raw: String,
    context: String,
    has_write_redirect: bool,
}

fn is_write_redirect(redir: &IoRedirect) -> bool {
    match redir {
        IoRedirect::File(_, kind, _) => matches!(
            kind,
            IoFileRedirectKind::Write
                | IoFileRedirectKind::Append
                | IoFileRedirectKind::Clobber
                | IoFileRedirectKind::ReadAndWrite
        ),
        IoRedirect::OutputAndError(_, _) => true,
        IoRedirect::HereDocument(_, _) | IoRedirect::HereString(_, _) => false,
    }
}

fn evaluate_single(
    program: &str,
    args: &[String],
    raw: &str,
    rules: &[SecurityRule],
) -> (PolicyDecision, Option<String>) {
    // Strip path prefix if someone invokes `/bin/rm` or `/usr/bin/cat`
    let base_program = program.rsplit('/').next().unwrap_or(program);
    let normalized_cmd = if args.is_empty() {
        base_program.to_string()
    } else {
        format!("{base_program} {}", args.join(" "))
    };

    let mut active_rules: Vec<&SecurityRule> = rules.iter().filter(|r| r.enabled).collect();
    // Higher priority first; on tie, stricter decision first
    active_rules.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then_with(|| b.decision.cmp(&a.decision))
    });

    for rule in active_rules {
        let matched = match rule.match_kind {
            RuleMatchKind::ProgramExact => base_program == rule.pattern.trim(),
            RuleMatchKind::CommandPrefix => {
                let pat = rule.pattern.trim();
                normalized_cmd == pat
                    || normalized_cmd.starts_with(&format!("{pat} "))
                    || raw.trim() == pat
                    || raw.trim().starts_with(&format!("{pat} "))
            }
            RuleMatchKind::Regex => Regex::new(&rule.pattern)
                .map(|re| re.is_match(raw) || re.is_match(&normalized_cmd))
                .unwrap_or(false),
        };

        if matched {
            return (rule.decision, Some(rule.name.clone()));
        }
    }

    // Default for unknown commands: require human-in-the-loop approval
    (PolicyDecision::AskApproval, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_safe_readonly_commands() {
        let engine = SecurityEngine::new();
        let rules = SecurityEngine::default_rules();

        let verdict = engine.evaluate("ls -la /var/log && df -h", &rules);
        assert!(verdict.parse_ok);
        assert_eq!(verdict.decision, PolicyDecision::Allow);
        assert_eq!(verdict.parsed_commands.len(), 2);
    }

    #[test]
    fn catches_pipeline_bypass_to_blacklist() {
        let engine = SecurityEngine::new();
        let rules = SecurityEngine::default_rules();

        let verdict = engine.evaluate("ls -la | rm -rf /tmp/test", &rules);
        assert!(verdict.parse_ok);
        assert_eq!(verdict.decision, PolicyDecision::Deny);
    }

    #[test]
    fn catches_command_substitution_bypass() {
        let engine = SecurityEngine::new();
        let rules = SecurityEngine::default_rules();

        let verdict = engine.evaluate("echo \"result: $(rm -rf /tmp/x)\"", &rules);
        assert!(verdict.parse_ok);
        assert_eq!(verdict.decision, PolicyDecision::Deny);
    }

    #[test]
    fn catches_backtick_substitution_bypass() {
        let engine = SecurityEngine::new();
        let rules = SecurityEngine::default_rules();

        let verdict = engine.evaluate("echo `reboot`", &rules);
        assert!(verdict.parse_ok);
        assert_eq!(verdict.decision, PolicyDecision::Deny);
    }

    #[test]
    fn requires_approval_for_unknown_or_mutating_commands() {
        let engine = SecurityEngine::new();
        let rules = SecurityEngine::default_rules();

        let verdict = engine.evaluate("systemctl restart nginx", &rules);
        assert!(verdict.parse_ok);
        assert_eq!(verdict.decision, PolicyDecision::AskApproval);
    }

    #[test]
    fn requires_approval_for_write_redirection() {
        let engine = SecurityEngine::new();
        let rules = SecurityEngine::default_rules();

        let verdict = engine.evaluate("echo hacked > /etc/hosts", &rules);
        assert!(verdict.parse_ok);
        assert_eq!(verdict.decision, PolicyDecision::AskApproval);
    }

    #[test]
    fn fail_closed_on_invalid_shell_syntax() {
        let engine = SecurityEngine::new();
        let rules = SecurityEngine::default_rules();

        let verdict = engine.evaluate("ls \"unterminated quote", &rules);
        assert!(!verdict.parse_ok);
        assert_eq!(verdict.decision, PolicyDecision::Deny);
    }

    #[test]
    fn strips_absolute_path_on_blacklisted_binary() {
        let engine = SecurityEngine::new();
        let rules = SecurityEngine::default_rules();

        let verdict = engine.evaluate("/bin/rm -rf /", &rules);
        assert!(verdict.parse_ok);
        assert_eq!(verdict.decision, PolicyDecision::Deny);
    }
}
