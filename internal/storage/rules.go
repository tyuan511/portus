package storage

import (
	"context"
	"database/sql"
	"errors"
	"fmt"

	"github.com/tyuan511/bulwark/internal/domain"
	"github.com/tyuan511/bulwark/internal/storage/db"
)

// --- security rules ---

func ruleToDomain(r db.SecurityRule) *domain.SecurityRule {
	return &domain.SecurityRule{
		ID:          r.ID,
		Name:        r.Name,
		MatchKind:   domain.RuleMatchKind(r.MatchKind),
		Pattern:     r.Pattern,
		Decision:    domain.PolicyDecision(r.Decision),
		Description: r.Description,
		Enabled:     r.Enabled == 1,
		Priority:    int(r.Priority),
	}
}

// UpsertRule inserts or updates an L1 security rule.
func (h *DB) UpsertRule(r *domain.SecurityRule) error {
	err := h.q.UpsertRule(context.Background(), db.UpsertRuleParams{
		ID:          r.ID,
		Name:        r.Name,
		MatchKind:   string(r.MatchKind),
		Pattern:     r.Pattern,
		Decision:    string(r.Decision),
		Description: r.Description,
		Enabled:     int32(boolToInt(r.Enabled)),
		Priority:    int32(r.Priority),
	})
	if err != nil {
		return fmt.Errorf("upsert rule: %w", err)
	}
	return nil
}

// ListRules returns all security rules ordered by priority (desc).
func (h *DB) ListRules() ([]*domain.SecurityRule, error) {
	rows, err := h.q.ListRules(context.Background())
	if err != nil {
		return nil, fmt.Errorf("list rules: %w", err)
	}
	out := make([]*domain.SecurityRule, 0, len(rows))
	for _, r := range rows {
		out = append(out, ruleToDomain(r))
	}
	return out, nil
}

// DeleteRule removes a rule; returns whether a row was deleted.
func (h *DB) DeleteRule(id string) (bool, error) {
	n, err := h.q.DeleteRule(context.Background(), id)
	if err != nil {
		return false, fmt.Errorf("delete rule: %w", err)
	}
	return n > 0, nil
}

// CountRules returns the number of stored rules (seed detection).
func (h *DB) CountRules() (int, error) {
	n, err := h.q.CountRules(context.Background())
	return int(n), err
}

// --- L2 config ---

// GetL2Config returns the active L2 provider configuration (default when unset).
func (h *DB) GetL2Config() (*domain.L2ProviderConfig, error) {
	row, err := h.q.GetL2Config(context.Background())
	if errors.Is(err, sql.ErrNoRows) {
		def := domain.DefaultL2ProviderConfig()
		return &def, nil
	}
	if err != nil {
		return nil, fmt.Errorf("get l2 config: %w", err)
	}
	return &domain.L2ProviderConfig{
		ID:                 row.ID,
		Name:               row.Name,
		Protocol:           domain.L2Protocol(row.Protocol),
		BaseURL:            row.BaseUrl,
		Model:              row.Model,
		APIKeyCredentialID: row.ApiKeyCredentialID,
		TimeoutMs:          row.TimeoutMs,
		RiskThresholdAsk:   int(row.RiskThresholdAsk),
		RiskThresholdDeny:  int(row.RiskThresholdDeny),
		Enabled:            row.Enabled == 1,
	}, nil
}

// UpsertL2Config saves the L2 provider configuration.
func (h *DB) UpsertL2Config(c *domain.L2ProviderConfig) error {
	err := h.q.UpsertL2Config(context.Background(), db.UpsertL2ConfigParams{
		ID:                 c.ID,
		Name:               c.Name,
		Protocol:           string(c.Protocol),
		BaseUrl:            c.BaseURL,
		Model:              c.Model,
		ApiKeyCredentialID: c.APIKeyCredentialID,
		TimeoutMs:          c.TimeoutMs,
		RiskThresholdAsk:   int32(c.RiskThresholdAsk),
		RiskThresholdDeny:  int32(c.RiskThresholdDeny),
		Enabled:            int32(boolToInt(c.Enabled)),
	})
	if err != nil {
		return fmt.Errorf("upsert l2 config: %w", err)
	}
	return nil
}

// --- snippets ---

// UpsertSnippet inserts or updates a command snippet.
func (h *DB) UpsertSnippet(s *domain.CommandSnippet) error {
	if s.Tags == nil {
		s.Tags = []string{}
	}
	err := h.q.UpsertSnippet(context.Background(), db.UpsertSnippetParams{
		ID:          s.ID,
		Name:        s.Name,
		Command:     s.Command,
		Description: s.Description,
		TagsJson:    tagsJSON(s.Tags),
	})
	if err != nil {
		return fmt.Errorf("upsert snippet: %w", err)
	}
	return nil
}

// ListSnippets returns all snippets.
func (h *DB) ListSnippets() ([]*domain.CommandSnippet, error) {
	rows, err := h.q.ListSnippets(context.Background())
	if err != nil {
		return nil, fmt.Errorf("list snippets: %w", err)
	}
	out := make([]*domain.CommandSnippet, 0, len(rows))
	for _, s := range rows {
		out = append(out, &domain.CommandSnippet{
			ID:          s.ID,
			Name:        s.Name,
			Command:     s.Command,
			Description: s.Description,
			Tags:        parseTags(s.TagsJson),
		})
	}
	return out, nil
}

// DeleteSnippet removes a snippet; returns whether a row was deleted.
func (h *DB) DeleteSnippet(id string) (bool, error) {
	n, err := h.q.DeleteSnippet(context.Background(), id)
	if err != nil {
		return false, fmt.Errorf("delete snippet: %w", err)
	}
	return n > 0, nil
}

// --- settings KV ---

// GetSetting returns a global setting value.
func (h *DB) GetSetting(key string) (string, bool, error) {
	v, err := h.q.GetSetting(context.Background(), key)
	if errors.Is(err, sql.ErrNoRows) {
		return "", false, nil
	}
	if err != nil {
		return "", false, fmt.Errorf("get setting: %w", err)
	}
	return v, true, nil
}

// SetSetting writes a global setting value.
func (h *DB) SetSetting(key, value string) error {
	err := h.q.SetSetting(context.Background(), db.SetSettingParams{
		Key:       key,
		Value:     value,
		UpdatedAt: nowRFC3339(),
	})
	if err != nil {
		return fmt.Errorf("set setting: %w", err)
	}
	return nil
}
