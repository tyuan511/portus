package sshpool

import (
	"github.com/pkg/sftp"

	"github.com/tyuan511/bulwark/internal/apierr"
	"github.com/tyuan511/bulwark/internal/domain"
)

// OpenSFTP returns an SFTP client multiplexed over the pooled SSH connection
// for the asset. Only the SFTP client itself should be closed; the underlying
// connection stays in the pool for other operations.
func (p *Pool) OpenSFTP(asset *domain.ServerAsset, all map[string]*domain.ServerAsset) (*sftp.Client, error) {
	if asset.JumpMode == domain.JumpModeSuSSH {
		return p.openSuSFTP(asset, all)
	}
	client, err := p.GetOrConnect(asset, all)
	if err != nil {
		return nil, err
	}
	sc, err := sftp.NewClient(client)
	if err != nil {
		// A dead pooled connection is the usual cause; force a reconnect next time.
		p.Invalidate(asset.ID)
		return nil, apierr.Wrap(apierr.CodeSSHError, 502, "open SFTP subsystem failed", err)
	}
	return sc, nil
}
