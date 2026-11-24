package httpserver

import (
	"github.com/gofiber/fiber/v2"

	"github.com/tyuan511/bulwark/internal/apierr"
	"github.com/tyuan511/bulwark/internal/domain"
	"github.com/tyuan511/bulwark/internal/httpserver/resp"
	"github.com/tyuan511/bulwark/internal/storage"
)

// registerCredentialRoutes mounts the sealed-credential manager. Plaintext
// secrets are accepted once at creation, sealed with the server key, and
// never returned by any API.
func (s *Server) registerCredentialRoutes(g fiber.Router) {
	cg := g.Group("/credentials", requirePerm(domain.PermHostManage))
	cg.Get("/", s.handleListCredentials)
	cg.Post("/", s.handleCreateCredential)
	cg.Patch("/:id", s.handleRenameCredential)
	cg.Delete("/:id", s.handleDeleteCredential)
}

func (s *Server) handleListCredentials(c *fiber.Ctx) error {
	metas, err := s.db.ListCredentials()
	if err != nil {
		return apierr.StorageErr(err)
	}
	return resp.OK(c, metas)
}

type createCredentialRequest struct {
	Kind     string `json:"kind"` // password | private_key | api_key
	Label    string `json:"label"`
	Username string `json:"username"`
	Secret   string `json:"secret"`
}

func (s *Server) handleCreateCredential(c *fiber.Ctx) error {
	var req createCredentialRequest
	if err := c.BodyParser(&req); err != nil {
		return apierr.BadRequest("invalid JSON body")
	}
	if req.Label == "" || req.Secret == "" {
		return apierr.Unprocessable("label and secret are required")
	}
	switch req.Kind {
	case storage.CredKindPassword, storage.CredKindPrivateKey, storage.CredKindAPIKey:
	case "":
		req.Kind = storage.CredKindPassword
	default:
		return apierr.Unprocessable("kind must be password, private_key or api_key")
	}

	id := storage.GenerateID("cred")
	actor := identityFrom(c)
	meta, err := s.db.CreateCredential(id, req.Kind, req.Label, req.Username, req.Secret, actor.Username)
	if err != nil {
		return err
	}
	s.auditAction(c, "credential.create", "store credential "+meta.Label, domain.DecisionAllow, "")
	return resp.Created(c, meta)
}

// handleRenameCredential updates a credential's label (secrets are immutable;
// rotate by creating a new credential and repointing hosts).
func (s *Server) handleRenameCredential(c *fiber.Ctx) error {
	var req struct {
		Label string `json:"label"`
	}
	if err := c.BodyParser(&req); err != nil || req.Label == "" {
		return apierr.Unprocessable("label is required")
	}
	ok, err := s.db.RenameCredential(c.Params("id"), req.Label)
	if err != nil {
		return apierr.StorageErr(err)
	}
	if !ok {
		return apierr.NotFoundf("credential not found")
	}
	s.auditAction(c, "credential.rename", "rename credential "+c.Params("id"), domain.DecisionAllow, "")
	return resp.OK(c, fiber.Map{"updated": true})
}

func (s *Server) handleDeleteCredential(c *fiber.Ctx) error {
	credID := c.Params("id")
	ok, err := s.db.DeleteCredential(credID)
	if err != nil {
		return apierr.StorageErr(err)
	}
	if !ok {
		return apierr.NotFoundf("credential not found")
	}
	s.auditAction(c, "credential.delete", "delete credential "+credID, domain.DecisionAllow, "")
	return resp.OK(c, fiber.Map{"deleted": true})
}
