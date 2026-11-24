package httpserver

import (
	"fmt"
	"strconv"
	"strings"

	sshcfg "github.com/kevinburke/ssh_config"

	"github.com/tyuan511/bulwark/internal/domain"
)

// importSSHConfigText parses an OpenSSH config file body into server assets.
// Wildcard hosts are skipped; HostName defaults to the Host pattern.
func importSSHConfigText(content string) ([]domain.ServerAsset, error) {
	if strings.TrimSpace(content) == "" {
		return nil, fmt.Errorf("empty ssh config content")
	}
	cfg, err := sshcfg.Decode(strings.NewReader(content))
	if err != nil {
		return nil, fmt.Errorf("parse ssh config: %w", err)
	}

	var assets []domain.ServerAsset
	seen := map[string]bool{}
	for _, host := range cfg.Hosts {
		patterns := hostPatterns(host)
		if len(patterns) == 0 || len(patterns) > 1 {
			continue // multi-pattern or empty blocks are ambiguous
		}
		pattern := patterns[0]
		if strings.ContainsAny(pattern, "*?!") || seen[pattern] {
			continue
		}

		hostName := cfgGetString(cfg, pattern, "HostName")
		if hostName == "" {
			hostName = pattern
		}
		user := cfgGetString(cfg, pattern, "User")
		if user == "" {
			user = "root"
		}
		port := 22
		if p := cfgGetString(cfg, pattern, "Port"); p != "" {
			if parsed, err := strconv.Atoi(p); err == nil && parsed > 0 && parsed < 65536 {
				port = parsed
			}
		}

		asset := domain.ServerAsset{
			ID:       "srv-" + sanitizeID(pattern),
			Name:     pattern,
			Host:     hostName,
			Port:     port,
			Username: user,
			AuthKind: domain.AuthAgentOrDefaultKey,
			Enabled:  true,
			Tags:     []string{},
		}
		if idf := cfgGetString(cfg, pattern, "IdentityFile"); idf != "" {
			asset.IdentityFile = idf
			asset.AuthKind = domain.AuthIdentityFile
		}
		// ProxyJump resolution happens after all assets are collected.
		if pj := cfgGetString(cfg, pattern, "ProxyJump"); pj != "" {
			asset.JumpHostID = "srv-" + sanitizeID(strings.Split(pj, ",")[0])
		}
		seen[pattern] = true
		assets = append(assets, asset)
	}

	// Drop ProxyJump links that point at hosts not present in this import.
	valid := map[string]bool{}
	for i := range assets {
		valid[assets[i].ID] = true
	}
	for i := range assets {
		if assets[i].JumpHostID != "" && !valid[assets[i].JumpHostID] {
			assets[i].JumpHostID = ""
		}
	}

	if len(assets) == 0 {
		return nil, fmt.Errorf("no concrete hosts found in ssh config")
	}
	return assets, nil
}

func hostPatterns(h *sshcfg.Host) []string {
	out := make([]string, 0, len(h.Patterns))
	for _, p := range h.Patterns {
		out = append(out, p.String())
	}
	return out
}

func cfgGetString(cfg *sshcfg.Config, pattern, key string) string {
	v, err := cfg.Get(pattern, key)
	if err != nil {
		return ""
	}
	return v
}

func sanitizeID(name string) string {
	var sb strings.Builder
	for _, r := range strings.ToLower(name) {
		switch {
		case r >= 'a' && r <= 'z', r >= '0' && r <= '9', r == '-', r == '_':
			sb.WriteRune(r)
		default:
			sb.WriteRune('-')
		}
	}
	out := sb.String()
	if len(out) > 40 {
		out = out[:40]
	}
	return strings.Trim(out, "-")
}
