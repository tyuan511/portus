// Package metrics exposes Prometheus runtime + business counters when
// metrics.enabled is set in the configuration.
package metrics

import (
	"net/http"

	"github.com/prometheus/client_golang/prometheus"
	"github.com/prometheus/client_golang/prometheus/promhttp"
)

var (
	// LoginsTotal counts login outcomes by result ("ok"|"failed"|"locked").
	LoginsTotal = prometheus.NewCounterVec(prometheus.CounterOpts{
		Namespace: "sshield",
		Name:      "logins_total",
		Help:      "Login attempts by outcome.",
	}, []string{"result"})

	// PolicyDenialsTotal counts commands blocked by policy (L1/L2), by channel.
	PolicyDenialsTotal = prometheus.NewCounterVec(prometheus.CounterOpts{
		Namespace: "sshield",
		Name:      "policy_denials_total",
		Help:      "Commands denied by the security pipeline, by channel (mcp/terminal).",
	}, []string{"channel"})

	// ApprovalsDecisionsTotal counts approval outcomes.
	ApprovalsDecisionsTotal = prometheus.NewCounterVec(prometheus.CounterOpts{
		Namespace: "sshield",
		Name:      "approval_decisions_total",
		Help:      "Approval decisions by outcome (approved/denied/timeout).",
	}, []string{"outcome"})

	// ExecutionsTotal counts command executions by channel and decision.
	ExecutionsTotal = prometheus.NewCounterVec(prometheus.CounterOpts{
		Namespace: "sshield",
		Name:      "executions_total",
		Help:      "Command executions by channel and final policy decision.",
	}, []string{"channel", "decision"})

	// FileOpsTotal counts SFTP operations by kind.
	FileOpsTotal = prometheus.NewCounterVec(prometheus.CounterOpts{
		Namespace: "sshield",
		Name:      "file_operations_total",
		Help:      "SFTP file operations by kind (upload/download/delete/mkdir/rename).",
	}, []string{"kind"})

	// ActiveTerminals gauges live interactive PTY sessions.
	ActiveTerminals = prometheus.NewGauge(prometheus.GaugeOpts{
		Namespace: "sshield",
		Name:      "active_terminals",
		Help:      "Currently open interactive terminal sessions.",
	})

	// PendingApprovals gauges approvals awaiting a decision.
	PendingApprovals = prometheus.NewGauge(prometheus.GaugeOpts{
		Namespace: "sshield",
		Name:      "pending_approvals",
		Help:      "Approval requests currently awaiting a decision.",
	})
)

func init() {
	// The default registry already exposes Go runtime + process collectors;
	// only the business metrics need explicit registration.
	prometheus.MustRegister(
		LoginsTotal,
		PolicyDenialsTotal,
		ApprovalsDecisionsTotal,
		ExecutionsTotal,
		FileOpsTotal,
		ActiveTerminals,
		PendingApprovals,
	)
}

// Handler serves the Prometheus text exposition format.
func Handler() http.Handler { return promhttp.Handler() }
