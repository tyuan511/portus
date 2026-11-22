package httpserver

import (
	"time"

	"github.com/gofiber/fiber/v2"

	"github.com/yuantang/sshield/internal/apierr"
	"github.com/yuantang/sshield/internal/domain"
	"github.com/yuantang/sshield/internal/httpserver/resp"
	"github.com/yuantang/sshield/internal/storage"
)

// registerServerRoutes mounts server-asset endpoints. app carries the
// unauthenticated group for route-shape symmetry; all handlers live behind
// the auth group.
func (s *Server) registerServerRoutes(app *fiber.App, authGroup fiber.Router) {
	authGroup.Get("/servers", s.handleListServers)
	authGroup.Post("/servers", requirePerm(domain.PermHostManage), s.handleUpsertServer)
	authGroup.Put("/servers/:id", requirePerm(domain.PermHostManage), s.handleUpdateServer)
	authGroup.Delete("/servers/:id", requirePerm(domain.PermHostManage), s.handleDeleteServer)
	authGroup.Post("/servers/:id/test", s.handleTestServer)
	authGroup.Post("/servers/import-ssh-config", requirePerm(domain.PermHostManage), s.handleImportSSHConfig)
	authGroup.Get("/servers/groups", s.handleListServerGroups)
}

// handleUpdateServer validates PUT against an existing host (unlike POST
// upsert, PUT must not silently create).
func (s *Server) handleUpdateServer(c *fiber.Ctx) error {
	id := c.Params("id")
	if _, err := s.db.GetServer(id); err != nil {
		return apierr.NotFoundf("server not found")
	}
	var asset domain.ServerAsset
	if err := c.BodyParser(&asset); err != nil {
		return apierr.BadRequest("invalid JSON body")
	}
	asset.ID = id
	if asset.Host == "" || asset.Name == "" || asset.Username == "" {
		return apierr.Unprocessable("name, host and username are required")
	}
	if asset.Port <= 0 || asset.Port > 65535 {
		asset.Port = 22
	}
	if asset.Tags == nil {
		asset.Tags = []string{}
	}
	if err := s.db.UpsertServer(&asset); err != nil {
		return apierr.StorageErr(err)
	}
	s.pool.Invalidate(asset.ID)
	s.auditAction(c, "server.update", "update server "+asset.Name, domain.DecisionAllow, "")
	return resp.OK(c, &asset)
}

func (s *Server) handleListServers(c *fiber.Ctx) error {
	id := identityFrom(c)
	if err := s.rbac.CheckPermission(id, domain.PermHostRead); err != nil {
		return err
	}
	needExecute := c.Query("capability") == "execute"
	servers, err := s.rbac.ListAccessibleServers(id, needExecute)
	if err != nil {
		return err
	}
	if id.IsAdmin {
		// Admin response includes auth metadata for the management UI.
		return resp.OK(c, servers)
	}
	out := make([]domain.AuthorizedServerInfo, 0, len(servers))
	for _, srv := range servers {
		out = append(out, domain.FromAsset(srv))
	}
	return resp.OK(c, out)
}

func (s *Server) handleListServerGroups(c *fiber.Ctx) error {
	id := identityFrom(c)
	if err := s.rbac.CheckPermission(id, domain.PermHostRead); err != nil {
		return err
	}
	groups, err := s.db.DistinctServerGroups()
	if err != nil {
		return apierr.StorageErr(err)
	}
	return resp.OK(c, groups)
}

func (s *Server) handleUpsertServer(c *fiber.Ctx) error {
	var asset domain.ServerAsset
	if err := c.BodyParser(&asset); err != nil {
		return apierr.BadRequest("invalid JSON body")
	}
	if asset.ID == "" {
		asset.ID = storage.GenerateID("srv")
		asset.Enabled = true // new hosts are active unless explicitly disabled later
	}
	if asset.Host == "" || asset.Name == "" || asset.Username == "" {
		return apierr.Unprocessable("name, host and username are required")
	}
	if asset.Port <= 0 || asset.Port > 65535 {
		asset.Port = 22
	}
	if asset.Tags == nil {
		asset.Tags = []string{}
	}
	if err := s.db.UpsertServer(&asset); err != nil {
		return apierr.StorageErr(err)
	}
	s.pool.Invalidate(asset.ID)

	s.auditAction(c, "server.upsert", "upsert server "+asset.Name, domain.DecisionAllow, "")
	return resp.OK(c, &asset)
}

func (s *Server) handleDeleteServer(c *fiber.Ctx) error {
	serverID := c.Params("id")
	asset, err := s.db.GetServer(serverID)
	if err != nil {
		return apierr.NotFoundf("server %s not found", serverID)
	}
	if ok, err := s.db.DeleteServer(serverID); err != nil {
		return apierr.StorageErr(err)
	} else if !ok {
		return apierr.NotFoundf("server %s not found", serverID)
	}
	s.pool.Invalidate(serverID)
	s.auditAction(c, "server.delete", "delete server "+asset.Name, domain.DecisionAllow, "")
	return resp.OK(c, fiber.Map{"deleted": true})
}

func (s *Server) handleTestServer(c *fiber.Ctx) error {
	serverID := c.Params("id")
	asset, err := s.db.GetServer(serverID)
	if err != nil {
		return apierr.NotFoundf("server %s not found", serverID)
	}
	if err := s.rbac.CheckServerAccess(identityFrom(c), asset, false); err != nil {
		return err
	}
	all, err := s.db.ListServers()
	if err != nil {
		return apierr.StorageErr(err)
	}
	assetMap := assetMap(all)
	if err := s.pool.TestConnection(asset, assetMap); err != nil {
		return resp.OK(c, fiber.Map{"ok": false, "message": err.Error()})
	}
	return resp.OK(c, fiber.Map{"ok": true, "message": "SSH connection & auth succeeded"})
}

type importSSHConfigRequest struct {
	Content string `json:"content"`
}

func (s *Server) handleImportSSHConfig(c *fiber.Ctx) error {
	var req importSSHConfigRequest
	if err := c.BodyParser(&req); err != nil {
		return apierr.BadRequest("invalid JSON body")
	}
	assets, err := importSSHConfigText(req.Content)
	if err != nil {
		return apierr.BadRequest(err.Error())
	}
	imported := make([]*domain.ServerAsset, 0, len(assets))
	for i := range assets {
		asset := &assets[i]
		existing, err := s.db.GetServer(asset.ID)
		if err == nil {
			// Preserve vault credential links on re-import.
			asset.CredentialID = existing.CredentialID
		} else {
			asset.ID = storage.GenerateID("srv")
		}
		if err := s.db.UpsertServer(asset); err != nil {
			return apierr.StorageErr(err)
		}
		imported = append(imported, asset)
	}
	return resp.OK(c, imported)
}

// --- shared helpers ---

func assetMap(all []*domain.ServerAsset) map[string]*domain.ServerAsset {
	m := make(map[string]*domain.ServerAsset, len(all))
	for _, a := range all {
		m[a.ID] = a
	}
	return m
}

// auditAction records an administrative action in the tamper-evident chain.
func (s *Server) auditAction(c *fiber.Ctx, command, intent string, decision domain.PolicyDecision, extra string) {
	id := identityFrom(c)
	ev := &domain.AuditEvent{
		ID:        newAuditID(),
		Actor:     "console",
		Timestamp: time.Now().UTC(),
		Command:   command,
		Intent:    intent,
		Decision:  decision,
		Operator:  c.IP(),
	}
	if id != nil {
		ev.UserID = id.UserID
		ev.Username = id.Username
	}
	if extra != "" {
		ev.Error = extra
	}
	s.Audit(ev)
}
