package storage

import (
	"database/sql"
	"fmt"

	"github.com/yuantang/sshield/internal/domain"
)

// AuditStatsFilter scopes statistics: optional user scoping mirrors the
// audit.view_self permission level.
type AuditStatsFilter struct {
	From     *string // RFC3339
	To       *string
	ServerID string
	UserID   string // empty = all users
}

func (f *AuditStatsFilter) where() (string, []any) {
	cond := " WHERE 1=1"
	var args []any
	if f.From != nil && *f.From != "" {
		cond += " AND timestamp >= ?"
		args = append(args, *f.From)
	}
	if f.To != nil && *f.To != "" {
		cond += " AND timestamp <= ?"
		args = append(args, *f.To)
	}
	if f.ServerID != "" {
		cond += " AND server_id = ?"
		args = append(args, f.ServerID)
	}
	if f.UserID != "" {
		cond += " AND user_id = ?"
		args = append(args, f.UserID)
	}
	return cond, args
}

// AuditStats computes the aggregated dashboard statistics.
func (db *DB) AuditStats(f *AuditStatsFilter) (*domain.AuditStats, error) {
	cond, args := f.where()
	stats := &domain.AuditStats{
		ByActor:     []domain.AuditCountBucket{},
		ByUser:      []domain.AuditCountBucket{},
		ByServer:    []domain.AuditServerBucket{},
		TopCommands: []domain.AuditCountBucket{},
		DailyTrend:  []domain.AuditDailyBucket{},
	}

	counts := []struct {
		dst *int64
		sel string
	}{
		{&stats.TotalCommands, "COUNT(*)"},
		{&stats.AllowedCount, "SUM(CASE WHEN decision = 'allow' THEN 1 ELSE 0 END)"},
		{&stats.ApprovalCount, "SUM(CASE WHEN approval_status = 'approved' THEN 1 ELSE 0 END)"},
		{&stats.DeniedCount, "SUM(CASE WHEN decision = 'deny' THEN 1 ELSE 0 END)"},
		{&stats.FailedCount, "SUM(CASE WHEN (exit_code IS NOT NULL AND exit_code != 0) OR (error IS NOT NULL AND error != '') THEN 1 ELSE 0 END)"},
	}
	for _, c := range counts {
		if err := db.sql.QueryRow(`SELECT `+c.sel+` FROM audit_logs`+cond, args...).Scan(c.dst); err != nil {
			return nil, fmt.Errorf("audit stats: %w", err)
		}
	}

	var avg sql.NullFloat64
	if err := db.sql.QueryRow(`SELECT AVG(duration_ms) FROM audit_logs`+cond+
		` AND duration_ms IS NOT NULL`, args...).Scan(&avg); err != nil && err != sql.ErrNoRows {
		return nil, err
	}
	if avg.Valid {
		stats.AvgDurationMs = &avg.Float64
	}

	byActor, err := db.countBuckets(`SELECT actor, COUNT(*) FROM audit_logs`+cond+
		` GROUP BY actor ORDER BY COUNT(*) DESC LIMIT 10`, args)
	if err != nil {
		return nil, err
	}
	stats.ByActor = byActor

	byUser, err := db.countBuckets(`SELECT COALESCE(NULLIF(username, ''), 'legacy'), COUNT(*)
		FROM audit_logs`+cond+` GROUP BY username ORDER BY COUNT(*) DESC LIMIT 10`, args)
	if err != nil {
		return nil, err
	}
	stats.ByUser = byUser

	byServer, err := db.serverBuckets(cond, args)
	if err != nil {
		return nil, err
	}
	stats.ByServer = byServer

	topCommands, err := db.countBuckets(`SELECT command, COUNT(*) FROM audit_logs`+cond+
		` GROUP BY command ORDER BY COUNT(*) DESC LIMIT 10`, args)
	if err != nil {
		return nil, err
	}
	stats.TopCommands = topCommands

	trend, err := db.dailyTrend(cond, args)
	if err != nil {
		return nil, err
	}
	stats.DailyTrend = trend
	return stats, nil
}

func (db *DB) countBuckets(q string, args []any) ([]domain.AuditCountBucket, error) {
	rows, err := db.sql.Query(q, args...)
	if err != nil {
		return nil, fmt.Errorf("stats bucket: %w", err)
	}
	defer rows.Close()
	out := []domain.AuditCountBucket{}
	for rows.Next() {
		var b domain.AuditCountBucket
		if err := rows.Scan(&b.Key, &b.Count); err != nil {
			return nil, err
		}
		out = append(out, b)
	}
	return out, rows.Err()
}

func (db *DB) serverBuckets(cond string, args []any) ([]domain.AuditServerBucket, error) {
	rows, err := db.sql.Query(`SELECT server_id, MAX(server_name), COUNT(*),
		SUM(CASE WHEN (exit_code IS NOT NULL AND exit_code != 0) OR (error IS NOT NULL AND error != '') THEN 1 ELSE 0 END)
		FROM audit_logs`+cond+` GROUP BY server_id ORDER BY COUNT(*) DESC LIMIT 20`, args...)
	if err != nil {
		return nil, fmt.Errorf("stats server: %w", err)
	}
	defer rows.Close()
	out := []domain.AuditServerBucket{}
	for rows.Next() {
		var b domain.AuditServerBucket
		if err := rows.Scan(&b.ServerID, &b.ServerName, &b.Count, &b.FailedCount); err != nil {
			return nil, err
		}
		out = append(out, b)
	}
	return out, rows.Err()
}

func (db *DB) dailyTrend(cond string, args []any) ([]domain.AuditDailyBucket, error) {
	// Timestamps are RFC3339 strings; substr() extracts the UTC date prefix.
	rows, err := db.sql.Query(`SELECT substr(timestamp, 1, 10) AS day, COUNT(*),
		SUM(CASE WHEN decision = 'deny' THEN 1 ELSE 0 END),
		SUM(CASE WHEN (exit_code IS NOT NULL AND exit_code != 0) OR (error IS NOT NULL AND error != '') THEN 1 ELSE 0 END)
		FROM audit_logs`+cond+` GROUP BY day ORDER BY day DESC LIMIT 30`, args...)
	if err != nil {
		return nil, fmt.Errorf("stats trend: %w", err)
	}
	defer rows.Close()
	out := []domain.AuditDailyBucket{}
	for rows.Next() {
		var b domain.AuditDailyBucket
		if err := rows.Scan(&b.Date, &b.Total, &b.Denied, &b.Failed); err != nil {
			return nil, err
		}
		out = append(out, b)
	}
	return out, rows.Err()
}
