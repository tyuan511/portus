// Package rbac evaluates permissions and host grants for identities.
package rbac

import (
	"github.com/tyuan511/bulwark/internal/apierr"
	"github.com/tyuan511/bulwark/internal/domain"
)

// Store is the storage surface rbac needs (implemented by storage.DB).
type Store interface {
	ListEnabledServers() ([]*domain.ServerAsset, error)
	ListServersForUserFiltered(userID string, needExecute bool) ([]*domain.ServerAsset, error)
	ListServersForUserTransferable(userID string) ([]*domain.ServerAsset, error)
	ServerGrantDecision(userID string, asset *domain.ServerAsset) (domain.GrantDecision, error)
}

// Service answers permission and host-access questions for identities.
type Service struct {
	store Store
}

func NewService(store Store) *Service { return &Service{store: store} }

// CheckPermission returns a structured 403 when the identity lacks perm.
func (s *Service) CheckPermission(id *domain.Identity, perm domain.Permission) error {
	if id == nil {
		return apierr.Unauthorized("")
	}
	if !id.HasPermission(perm) {
		return apierr.Forbidden("missing permission: " + string(perm))
	}
	return nil
}

// HasPermission is the boolean flavor.
func HasPermission(id *domain.Identity, perm domain.Permission) bool {
	return id != nil && id.HasPermission(perm)
}

// ListAccessibleServers returns servers visible to the identity, applying
// admin bypass or grant filtering. needExecute narrows to execute-granted hosts.
func (s *Service) ListAccessibleServers(id *domain.Identity, needExecute bool) ([]*domain.ServerAsset, error) {
	if id.IsAdmin {
		// Admins bypass grants and see every enabled server.
		return s.store.ListEnabledServers()
	}
	return s.store.ListServersForUserFiltered(id.UserID, needExecute)
}

// CheckServerAccess verifies the identity may connect (or execute) on asset.
func (s *Service) CheckServerAccess(id *domain.Identity, asset *domain.ServerAsset, needExecute bool) error {
	if err := s.CheckPermission(id, permFor(needExecute)); err != nil {
		return err
	}
	if id.IsAdmin {
		return nil
	}
	decision, err := s.store.ServerGrantDecision(id.UserID, asset)
	if err != nil {
		return apierr.StorageErr(err)
	}
	if needExecute {
		if !decision.CanExecute {
			return apierr.Forbidden("no execute grant on host " + asset.Name)
		}
		return nil
	}
	if !decision.CanConnect {
		return apierr.Forbidden("no connect grant on host " + asset.Name)
	}
	return nil
}

// CheckServerTransfer verifies the identity may run SFTP file operations on
// asset: the host.transfer permission plus a can_transfer grant (admins
// bypass grants).
func (s *Service) CheckServerTransfer(id *domain.Identity, asset *domain.ServerAsset) error {
	if err := s.CheckPermission(id, domain.PermHostTransfer); err != nil {
		return err
	}
	if id.IsAdmin {
		return nil
	}
	decision, err := s.store.ServerGrantDecision(id.UserID, asset)
	if err != nil {
		return apierr.StorageErr(err)
	}
	if !decision.CanTransfer {
		return apierr.Forbidden("no transfer grant on host " + asset.Name)
	}
	return nil
}

// ListTransferableServers returns hosts the identity may transfer files on.
func (s *Service) ListTransferableServers(id *domain.Identity) ([]*domain.ServerAsset, error) {
	if id.IsAdmin {
		return s.store.ListEnabledServers()
	}
	return s.store.ListServersForUserTransferable(id.UserID)
}

// CanSkipTerminalApproval reports whether id is explicitly granted
// terminal_no_approval on asset. Defaults to false (require approval) when not
// configured or on any lookup error (fail-closed).
func (s *Service) CanSkipTerminalApproval(id *domain.Identity, asset *domain.ServerAsset) bool {
	if id == nil || asset == nil || id.IsAdmin {
		return false
	}
	decision, err := s.store.ServerGrantDecision(id.UserID, asset)
	if err != nil {
		return false
	}
	return decision.CanConnect && decision.TerminalNoApproval
}

// CanSkipMCPApproval reports whether id is explicitly granted mcp_no_approval
// on asset. Defaults to false (require approval) when not configured or on any
// lookup error (fail-closed).
func (s *Service) CanSkipMCPApproval(id *domain.Identity, asset *domain.ServerAsset) bool {
	if id == nil || asset == nil || id.IsAdmin {
		return false
	}
	decision, err := s.store.ServerGrantDecision(id.UserID, asset)
	if err != nil {
		return false
	}
	return decision.CanExecute && decision.MCPNoApproval
}

func permFor(needExecute bool) domain.Permission {
	if needExecute {
		return domain.PermHostExecute
	}
	return domain.PermHostConnect
}
