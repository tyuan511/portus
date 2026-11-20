package storage

import (
	"context"
	"fmt"

	"github.com/yuantang/sshield/internal/domain"
	"github.com/yuantang/sshield/internal/storage/db"
)

// GenerateID produces a short prefixed identifier (e.g. "srv-a1b2c3d4").
func GenerateID(prefix string) string {
	return prefix + "-" + shortID()
}

// AuditStatsFilter scopes statistics. UserID mirrors the audit.view_self level.
type AuditStatsFilter struct {
	From     *string // RFC3339
	To       *string
	ServerID string
	UserID   string
}

// AuditStats computes the aggregated dashboard statistics via sqlc queries.
func (h *DB) AuditStats(f *AuditStatsFilter) (*domain.AuditStats, error) {
	ctx := context.Background()

	stats := &domain.AuditStats{
		ByActor:     []domain.AuditCountBucket{},
		ByUser:      []domain.AuditCountBucket{},
		ByServer:    []domain.AuditServerBucket{},
		TopCommands: []domain.AuditCountBucket{},
		DailyTrend:  []domain.AuditDailyBucket{},
	}

	totals, err := h.q.AuditStatsTotals(ctx, f.totalsParams())
	if err != nil {
		return nil, fmt.Errorf("audit stats totals: %w", err)
	}
	stats.TotalCommands = totals.TotalCommands
	stats.AllowedCount = totals.AllowedCount
	stats.ApprovalCount = totals.ApprovalCount
	stats.DeniedCount = totals.DeniedCount
	stats.FailedCount = totals.FailedCount
	avg := totals.AvgDurationMs
	stats.AvgDurationMs = &avg

	actors, err := h.q.AuditStatsByActor(ctx, f.rangeParams())
	if err != nil {
		return nil, fmt.Errorf("audit stats by actor: %w", err)
	}
	for _, r := range actors {
		stats.ByActor = append(stats.ByActor, domain.AuditCountBucket{Key: r.Key, Count: r.Count})
	}

	users, err := h.q.AuditStatsByUser(ctx, f.byUserParams())
	if err != nil {
		return nil, fmt.Errorf("audit stats by user: %w", err)
	}
	for _, r := range users {
		stats.ByUser = append(stats.ByUser, domain.AuditCountBucket{Key: r.Key, Count: r.Count})
	}

	servers, err := h.q.AuditStatsByServer(ctx, f.byServerParams())
	if err != nil {
		return nil, fmt.Errorf("audit stats by server: %w", err)
	}
	for _, r := range servers {
		stats.ByServer = append(stats.ByServer, domain.AuditServerBucket{
			ServerID: r.ServerID, ServerName: r.ServerName, Count: r.Count, FailedCount: r.FailedCount,
		})
	}

	commands, err := h.q.AuditStatsTopCommands(ctx, f.byCommandsParams())
	if err != nil {
		return nil, fmt.Errorf("audit stats top commands: %w", err)
	}
	for _, r := range commands {
		stats.TopCommands = append(stats.TopCommands, domain.AuditCountBucket{Key: r.Key, Count: r.Count})
	}

	trend, err := h.q.AuditStatsDailyTrend(ctx, f.byTrendParams())
	if err != nil {
		return nil, fmt.Errorf("audit stats trend: %w", err)
	}
	for _, r := range trend {
		stats.DailyTrend = append(stats.DailyTrend, domain.AuditDailyBucket{
			Date: r.Date, Total: r.Total, Denied: r.Denied, Failed: r.Failed,
		})
	}
	return stats, nil
}

func (f *AuditStatsFilter) totalsParams() db.AuditStatsTotalsParams {
	return db.AuditStatsTotalsParams{
		ServerID: nullString(f.ServerID),
		UserID:   nullString(f.UserID),
		FromTs:   nullString(deref(f.From)),
		ToTs:     nullString(deref(f.To)),
	}
}

func (f *AuditStatsFilter) rangeParams() db.AuditStatsByActorParams {
	return db.AuditStatsByActorParams{
		ServerID: nullString(f.ServerID),
		UserID:   nullString(f.UserID),
		FromTs:   nullString(deref(f.From)),
		ToTs:     nullString(deref(f.To)),
	}
}

func (f *AuditStatsFilter) byUserParams() db.AuditStatsByUserParams {
	return db.AuditStatsByUserParams{
		ServerID: nullString(f.ServerID),
		UserID:   nullString(f.UserID),
		FromTs:   nullString(deref(f.From)),
		ToTs:     nullString(deref(f.To)),
	}
}

func (f *AuditStatsFilter) byServerParams() db.AuditStatsByServerParams {
	return db.AuditStatsByServerParams{
		ServerID: nullString(f.ServerID),
		UserID:   nullString(f.UserID),
		FromTs:   nullString(deref(f.From)),
		ToTs:     nullString(deref(f.To)),
	}
}

func (f *AuditStatsFilter) byCommandsParams() db.AuditStatsTopCommandsParams {
	return db.AuditStatsTopCommandsParams{
		ServerID: nullString(f.ServerID),
		UserID:   nullString(f.UserID),
		FromTs:   nullString(deref(f.From)),
		ToTs:     nullString(deref(f.To)),
	}
}

func (f *AuditStatsFilter) byTrendParams() db.AuditStatsDailyTrendParams {
	return db.AuditStatsDailyTrendParams{
		ServerID: nullString(f.ServerID),
		UserID:   nullString(f.UserID),
		FromTs:   nullString(deref(f.From)),
		ToTs:     nullString(deref(f.To)),
	}
}

func deref(s *string) string {
	if s == nil {
		return ""
	}
	return *s
}
