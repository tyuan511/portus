// Package notify pushes operational events (approval requests/decisions,
// policy denials, account lockouts) to a configured webhook. Delivery is
// fire-and-forget: failures are logged, never propagated to callers.
package notify

import (
	"bytes"
	"encoding/json"
	"fmt"
	"net/http"
	"time"

	"go.uber.org/zap"
)

// Notifier posts JSON payloads to the configured webhook URL.
type Notifier struct {
	url    string
	client *http.Client
	log    *zap.Logger
}

// New builds a Notifier; an empty URL disables all deliveries.
func New(url string, timeoutSecs int, log *zap.Logger) *Notifier {
	if timeoutSecs <= 0 {
		timeoutSecs = 5
	}
	return &Notifier{
		url:    url,
		client: &http.Client{Timeout: time.Duration(timeoutSecs) * time.Second},
		log:    log,
	}
}

// Enabled reports whether a webhook destination is configured.
func (n *Notifier) Enabled() bool { return n != nil && n.url != "" }

// Event is the webhook payload. Type is a stable dot-separated identifier,
// e.g. "approval.requested".
type Event struct {
	Type   string         `json:"type"`
	Time   string         `json:"time"`
	Summary string        `json:"summary"`
	Fields map[string]any `json:"fields,omitempty"`
}

// Send posts the event asynchronously. Safe to call from any goroutine; a
// nil or disabled notifier is a no-op.
func (n *Notifier) Send(typ, summary string, fields map[string]any) {
	if !n.Enabled() {
		return
	}
	ev := Event{
		Type:    typ,
		Time:    time.Now().UTC().Format(time.RFC3339),
		Summary: summary,
		Fields:  fields,
	}
	go func() {
		body, err := json.Marshal(ev)
		if err != nil {
			return
		}
		resp, err := n.client.Post(n.url, "application/json", bytes.NewReader(body))
		if err != nil {
			n.log.Warn("webhook notify failed", zap.String("type", typ), zap.Error(err))
			return
		}
		defer resp.Body.Close()
		if resp.StatusCode >= 300 {
			n.log.Warn("webhook notify rejected",
				zap.String("type", typ), zap.Int("status", resp.StatusCode))
		}
	}()
}

// TruncateCommand caps long commands in webhook payloads.
func TruncateCommand(s string) string {
	if len(s) <= 200 {
		return s
	}
	return fmt.Sprintf("%s…[%d chars]", s[:200], len(s))
}
