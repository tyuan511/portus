// Package approvals brokers human-in-the-loop command approvals with live
// websocket fan-out to privileged sessions.
package approvals

import (
	"sync"
	"time"

	"github.com/tyuan511/bulwark/internal/domain"
)

// Event is a websocket-pushed approval event.
type Event struct {
	Type    string                  `json:"type"` // approval_requested | approval_resolved
	Payload *domain.ApprovalRequest `json:"payload,omitempty"`
	// Resolved carries the decision for approval_resolved events.
	ApprovalID string                  `json:"approval_id,omitempty"`
	Decision   domain.ApprovalDecision `json:"decision,omitempty"`
}

type pending struct {
	req      domain.ApprovalRequest
	decision chan domain.ApprovalDecision
}

type subscriber struct {
	ch     chan Event
	filter func(*domain.ApprovalRequest) bool
}

// Broker tracks pending approvals, waits for decisions with timeout, and
// broadcasts events to subscribers.
type Broker struct {
	mu      sync.Mutex
	pending map[string]*pending
	subs    map[*subscriber]struct{}
}

func NewBroker() *Broker {
	return &Broker{
		pending: map[string]*pending{},
		subs:    map[*subscriber]struct{}{},
	}
}

// Subscribe registers an event stream. filter decides which approval events
// the subscriber may see (permission gating).
func (b *Broker) Subscribe(filter func(*domain.ApprovalRequest) bool) (<-chan Event, func()) {
	ch := make(chan Event, 32)
	sub := &subscriber{ch: ch, filter: filter}
	b.mu.Lock()
	b.subs[sub] = struct{}{}
	b.mu.Unlock()
	cancel := func() {
		b.mu.Lock()
		delete(b.subs, sub)
		b.mu.Unlock()
	}
	return ch, cancel
}

func (b *Broker) broadcast(ev Event) {
	b.mu.Lock()
	defer b.mu.Unlock()
	for sub := range b.subs {
		if ev.Payload != nil && sub.filter != nil && !sub.filter(ev.Payload) {
			continue
		}
		select {
		case sub.ch <- ev:
		default: // slow consumer: drop rather than block the approval flow
		}
	}
}

// Request registers an approval and waits for a human decision or timeout.
func (b *Broker) Request(req domain.ApprovalRequest, timeout time.Duration) domain.ApprovalDecision {
	p := &pending{req: req, decision: make(chan domain.ApprovalDecision, 1)}

	b.mu.Lock()
	b.pending[req.ID] = p
	b.mu.Unlock()
	posted := domain.ApprovalRequest(req)
	b.broadcast(Event{Type: "approval_requested", Payload: &posted})

	timer := time.NewTimer(timeout)
	defer timer.Stop()

	select {
	case d := <-p.decision:
		return d
	case <-timer.C:
		b.resolve(req.ID, domain.ApprovalTimedOut)
		return domain.ApprovalTimedOut
	}
}

// Decide resolves a pending approval.
func (b *Broker) Decide(id string, approved bool) (domain.ApprovalDecision, bool) {
	return b.resolve(id, map[bool]domain.ApprovalDecision{true: domain.ApprovalApproved, false: domain.ApprovalRejected}[approved])
}

func (b *Broker) resolve(id string, decision domain.ApprovalDecision) (domain.ApprovalDecision, bool) {
	b.mu.Lock()
	p, ok := b.pending[id]
	if ok {
		delete(b.pending, id)
	}
	b.mu.Unlock()
	if !ok {
		return "", false
	}
	p.decision <- decision
	b.broadcast(Event{Type: "approval_resolved", ApprovalID: id, Decision: decision})
	return decision, true
}

// ListPending returns all unresolved approval requests.
func (b *Broker) ListPending() []domain.ApprovalRequest {
	b.mu.Lock()
	defer b.mu.Unlock()
	out := make([]domain.ApprovalRequest, 0, len(b.pending))
	for _, p := range b.pending {
		out = append(out, p.req)
	}
	return out
}

// GetPending fetches one pending request by id.
func (b *Broker) GetPending(id string) (domain.ApprovalRequest, bool) {
	b.mu.Lock()
	defer b.mu.Unlock()
	p, ok := b.pending[id]
	if !ok {
		return domain.ApprovalRequest{}, false
	}
	return p.req, true
}
