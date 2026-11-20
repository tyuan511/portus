package security

import (
	"fmt"

	"github.com/yuantang/sshield/internal/domain"
)

// DefaultRules returns the built-in starter rule set: deny destructive
// programs (priority 100), deny high-risk prefixes (95), allow read-only
// prefixes (50), allow read-only programs (10).
func DefaultRules() []*domain.SecurityRule {
	allowPrograms := []string{
		"ls", "ll", "pwd", "whoami", "id", "uname", "uptime", "date", "df", "du", "free",
		"top", "htop", "ps", "pgrep", "ss", "netstat", "ip", "ifconfig", "ping", "dig",
		"nslookup", "cat", "head", "tail", "less", "grep", "egrep", "rg", "awk", "sed",
		"wc", "sort", "uniq", "find", "which", "whereis", "echo", "printf", "env", "printenv",
		"journalctl",
	}

	allowPrefixes := []string{
		"docker ps", "docker logs", "docker inspect", "docker stats", "docker images",
		"kubectl get", "kubectl describe", "kubectl logs",
		"systemctl status", "git status", "git log", "git diff", "nginx -t",
	}

	denyPrograms := []string{
		"rm", "rmdir", "mkfs", "fdisk", "parted", "dd", "shutdown", "reboot", "poweroff",
		"halt", "init", "telinit", "userdel", "groupdel", "passwd", "chpasswd", "visudo",
		"iptables", "nft", "ufw",
	}

	denyPrefixes := []string{
		"chmod -R 777", "chown -R", "kill -9 1",
		"docker rm", "docker rmi", "docker system prune", "kubectl delete",
	}

	var rules []*domain.SecurityRule
	idx := 1
	add := func(kind domain.RuleMatchKind, pattern string, decision domain.PolicyDecision, priority int, desc string) {
		rules = append(rules, &domain.SecurityRule{
			ID:          fmt.Sprintf("builtin-%s-%d", map[domain.PolicyDecision]string{domain.DecisionDeny: "deny", domain.DecisionAllow: "allow"}[decision], idx),
			Name:        fmt.Sprintf("%s `%s`", map[domain.PolicyDecision]string{domain.DecisionDeny: "Block", domain.DecisionAllow: "Allow"}[decision], pattern),
			MatchKind:   kind,
			Pattern:     pattern,
			Decision:    decision,
			Description: desc,
			Enabled:     true,
			Priority:    priority,
		})
		idx++
	}

	for _, p := range denyPrograms {
		add(domain.MatchProgramExact, p, domain.DecisionDeny, 100, "Built-in destructive command blacklist")
	}
	for _, p := range denyPrefixes {
		add(domain.MatchCommandPrefix, p, domain.DecisionDeny, 95, "Built-in high-risk prefix blacklist")
	}
	for _, p := range allowPrefixes {
		add(domain.MatchCommandPrefix, p, domain.DecisionAllow, 50, "Built-in read-only diagnostic prefix")
	}
	for _, p := range allowPrograms {
		add(domain.MatchProgramExact, p, domain.DecisionAllow, 10, "Built-in read-only command whitelist")
	}
	return rules
}
