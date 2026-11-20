package httpserver

import (
	"time"

	"github.com/gofiber/fiber/v2"

	"github.com/yuantang/sshield/internal/apierr"
	"github.com/yuantang/sshield/internal/auth"
	"github.com/yuantang/sshield/internal/domain"
	"github.com/yuantang/sshield/internal/httpserver/resp"
	"github.com/yuantang/sshield/internal/storage"
)

func (s *Server) registerAdminRoutes(g fiber.Router) {
	// Users.
	g.Get("/admin/users", requirePerm(domain.PermUsersManage), s.handleListUsers)
	g.Post("/admin/users", requirePerm(domain.PermUsersManage), s.handleCreateUser)
	g.Patch("/admin/users/:id", requirePerm(domain.PermUsersManage), s.handlePatchUser)
	g.Delete("/admin/users/:id", requirePerm(domain.PermUsersManage), s.handleDeleteUser)
	g.Post("/admin/users/:id/reset-password", requirePerm(domain.PermUsersManage), s.handleResetPassword)
	g.Post("/admin/users/:id/reset-totp", requirePerm(domain.PermUsersManage), s.handleResetTOTP)

	// Roles.
	g.Get("/admin/roles", requirePerm(domain.PermRolesManage), s.handleListRoles)
	g.Post("/admin/roles", requirePerm(domain.PermRolesManage), s.handleCreateRole)
	g.Patch("/admin/roles/:id", requirePerm(domain.PermRolesManage), s.handlePatchRole)
	g.Delete("/admin/roles/:id", requirePerm(domain.PermRolesManage), s.handleDeleteRole)

	// User groups.
	g.Get("/admin/groups", requirePerm(domain.PermGroupsManage), s.handleListGroups)
	g.Post("/admin/groups", requirePerm(domain.PermGroupsManage), s.handleCreateGroup)
	g.Patch("/admin/groups/:id", requirePerm(domain.PermGroupsManage), s.handlePatchGroup)
	g.Delete("/admin/groups/:id", requirePerm(domain.PermGroupsManage), s.handleDeleteGroup)
	g.Get("/admin/groups/:id/members", requirePerm(domain.PermGroupsManage), s.handleListGroupMembers)
	g.Post("/admin/groups/:id/members", requirePerm(domain.PermGroupsManage), s.handleAddGroupMember)
	g.Delete("/admin/groups/:id/members/:userID", requirePerm(domain.PermGroupsManage), s.handleRemoveGroupMember)

	// Host access grants.
	g.Get("/admin/access-grants", requirePerm(domain.PermAccessManage), s.handleListGrants)
	g.Post("/admin/access-grants", requirePerm(domain.PermAccessManage), s.handleCreateGrant)
	g.Patch("/admin/access-grants/:id", requirePerm(domain.PermAccessManage), s.handlePatchGrant)
	g.Delete("/admin/access-grants/:id", requirePerm(domain.PermAccessManage), s.handleDeleteGrant)

	// Session administration.
	g.Get("/admin/sessions", requirePerm(domain.PermUsersManage), s.handleListAllSessions)
	g.Delete("/admin/sessions/:id", requirePerm(domain.PermUsersManage), s.handleKickSession)

	// Approvals.
	g.Get("/approvals", requirePerm(domain.PermApprovalsDecide), s.handleListApprovals)
	g.Post("/approvals/:id/decide", requirePerm(domain.PermApprovalsDecide), s.handleDecideApproval)
}

// --- users ---

func (s *Server) handleListUsers(c *fiber.Ctx) error {
	users, err := s.db.ListUsers()
	if err != nil {
		return apierr.StorageErr(err)
	}
	return resp.OK(c, users)
}

type createUserRequest struct {
	Username    string `json:"username"`
	DisplayName string `json:"display_name"`
	Password    string `json:"password"`
	RoleID      string `json:"role_id"`
}

func (s *Server) handleCreateUser(c *fiber.Ctx) error {
	var req createUserRequest
	if err := c.BodyParser(&req); err != nil {
		return apierr.BadRequest("invalid JSON body")
	}
	if req.Username == "" || req.RoleID == "" {
		return apierr.Unprocessable("username and role_id are required")
	}
	if err := auth.CheckPasswordPolicy(req.Password, req.Username); err != nil {
		return err
	}
	if _, err := s.db.GetRole(req.RoleID); err != nil {
		return apierr.NotFoundf("role %s not found", req.RoleID)
	}
	u := &domain.User{
		ID:          storage.GenerateID("usr"),
		Username:    req.Username,
		DisplayName: req.DisplayName,
		RoleID:      req.RoleID,
		IsActive:    true,
	}
	if err := s.db.CreateUser(u, auth.HashPassword(req.Password), "", nil); err != nil {
		return apierr.Conflict(err.Error())
	}
	created, err := s.db.GetUser(u.ID)
	if err != nil {
		return apierr.StorageErr(err)
	}
	s.auditAction(c, "user.create", "create user "+req.Username, domain.DecisionAllow, "")
	return resp.Created(c, created)
}

type patchUserRequest struct {
	DisplayName        *string `json:"display_name"`
	RoleID             *string `json:"role_id"`
	IsActive           *bool   `json:"is_active"`
	MustChangePassword *bool   `json:"must_change_password"`
}

func (s *Server) handlePatchUser(c *fiber.Ctx) error {
	var req patchUserRequest
	if err := c.BodyParser(&req); err != nil {
		return apierr.BadRequest("invalid JSON body")
	}
	id := c.Params("id")
	actor := identityFrom(c)
	if id == actor.UserID && req.IsActive != nil && !*req.IsActive {
		return apierr.Unprocessable("you cannot deactivate your own account")
	}
	if req.RoleID != nil {
		if _, err := s.db.GetRole(*req.RoleID); err != nil {
			return apierr.NotFoundf("role %s not found", *req.RoleID)
		}
	}
	if err := s.db.UpdateUserProfile(id, req.DisplayName, req.RoleID, req.IsActive, req.MustChangePassword); err != nil {
		if err == storage.ErrNotFound {
			return apierr.NotFoundf("user not found")
		}
		return apierr.StorageErr(err)
	}
	// Deactivation and role changes revoke live sessions immediately.
	if req.IsActive != nil && !*req.IsActive {
		_, _ = s.db.DeleteSessionsForUser(id)
	}
	if req.RoleID != nil {
		_, _ = s.db.DeleteSessionsForUser(id)
	}
	updated, err := s.db.GetUser(id)
	if err != nil {
		return apierr.StorageErr(err)
	}
	s.auditAction(c, "user.update", "update user "+updated.Username, domain.DecisionAllow, "")
	return resp.OK(c, updated)
}

func (s *Server) handleDeleteUser(c *fiber.Ctx) error {
	id := c.Params("id")
	actor := identityFrom(c)
	if id == actor.UserID {
		return apierr.Unprocessable("you cannot delete your own account")
	}
	ok, err := s.db.DeleteUser(id)
	if err != nil {
		return apierr.StorageErr(err)
	}
	if !ok {
		return apierr.NotFoundf("user not found")
	}
	s.auditAction(c, "user.delete", "delete user", domain.DecisionAllow, "")
	return resp.OK(c, fiber.Map{"deleted": true})
}

type resetPasswordRequest struct {
	NewPassword string `json:"new_password"` // empty → generate
}

func (s *Server) handleResetPassword(c *fiber.Ctx) error {
	var req resetPasswordRequest
	_ = c.BodyParser(&req)
	id := c.Params("id")
	user, err := s.db.GetUser(id)
	if err != nil {
		return apierr.NotFoundf("user not found")
	}
	temp, err := s.authSvc.ResetPasswordByAdmin(id, user.Username, req.NewPassword)
	if err != nil {
		return err
	}
	s.auditAction(c, "user.reset_password", "reset password for "+user.Username, domain.DecisionAllow, "")
	return resp.OK(c, fiber.Map{"temporary_password": temp, "must_change_password": true})
}

func (s *Server) handleResetTOTP(c *fiber.Ctx) error {
	id := c.Params("id")
	user, err := s.db.GetUser(id)
	if err != nil {
		return apierr.NotFoundf("user not found")
	}
	if err := s.db.SetUserTOTP(id, "", false, nil); err != nil {
		return apierr.StorageErr(err)
	}
	s.auditAction(c, "user.reset_totp", "reset TOTP for "+user.Username, domain.DecisionAllow, "")
	return resp.OK(c, fiber.Map{"reset": true})
}

// --- roles ---

func (s *Server) handleListRoles(c *fiber.Ctx) error {
	roles, err := s.db.ListRoles()
	if err != nil {
		return apierr.StorageErr(err)
	}
	return resp.OK(c, roles)
}

type roleRequest struct {
	Name        string              `json:"name"`
	Permissions []domain.Permission `json:"permissions"`
	Description string              `json:"description"`
}

func (s *Server) handleCreateRole(c *fiber.Ctx) error {
	var req roleRequest
	if err := c.BodyParser(&req); err != nil {
		return apierr.BadRequest("invalid JSON body")
	}
	if req.Name == "" {
		return apierr.Unprocessable("name is required")
	}
	if err := validatePermissions(req.Permissions); err != nil {
		return err
	}
	role := &domain.Role{
		ID:          storage.GenerateID("role"),
		Name:        req.Name,
		Permissions: req.Permissions,
		Description: req.Description,
		CreatedAt:   time.Now().UTC(),
	}
	if err := s.db.UpsertRole(role); err != nil {
		return apierr.StorageErr(err)
	}
	return resp.Created(c, role)
}

func (s *Server) handlePatchRole(c *fiber.Ctx) error {
	var req roleRequest
	if err := c.BodyParser(&req); err != nil {
		return apierr.BadRequest("invalid JSON body")
	}
	id := c.Params("id")
	existing, err := s.db.GetRole(id)
	if err != nil {
		return apierr.NotFoundf("role not found")
	}
	if existing.IsBuiltin && existing.Name == domain.BuiltinRoleAdmin {
		return apierr.Forbidden("the admin role is immutable")
	}
	if req.Permissions != nil {
		if err := validatePermissions(req.Permissions); err != nil {
			return err
		}
		existing.Permissions = req.Permissions
	}
	if req.Name != "" {
		existing.Name = req.Name
	}
	existing.Description = req.Description
	if err := s.db.UpsertRole(existing); err != nil {
		return apierr.StorageErr(err)
	}
	s.auditAction(c, "role.update", "update role "+existing.Name, domain.DecisionAllow, "")
	return resp.OK(c, existing)
}

func (s *Server) handleDeleteRole(c *fiber.Ctx) error {
	ok, err := s.db.DeleteRole(c.Params("id"))
	if err != nil {
		if err == storage.ErrNotFound {
			return apierr.NotFoundf("role not found")
		}
		return apierr.Conflict(err.Error())
	}
	return resp.OK(c, fiber.Map{"deleted": ok})
}

func validatePermissions(perms []domain.Permission) error {
	valid := map[domain.Permission]bool{}
	for _, p := range domain.AllPermissions() {
		valid[p] = true
	}
	for _, p := range perms {
		if !valid[p] {
			return apierr.Unprocessable("unknown permission: " + string(p))
		}
	}
	return nil
}

// --- user groups ---

func (s *Server) handleListGroups(c *fiber.Ctx) error {
	groups, err := s.db.ListUserGroups()
	if err != nil {
		return apierr.StorageErr(err)
	}
	return resp.OK(c, groups)
}

type groupRequest struct {
	Name        string `json:"name"`
	Description string `json:"description"`
}

func (s *Server) handleCreateGroup(c *fiber.Ctx) error {
	var req groupRequest
	if err := c.BodyParser(&req); err != nil {
		return apierr.BadRequest("invalid JSON body")
	}
	if req.Name == "" {
		return apierr.Unprocessable("name is required")
	}
	g := &domain.UserGroup{ID: storage.GenerateID("grp"), Name: req.Name, Description: req.Description}
	if err := s.db.CreateUserGroup(g); err != nil {
		return apierr.Conflict(err.Error())
	}
	return resp.Created(c, g)
}

func (s *Server) handlePatchGroup(c *fiber.Ctx) error {
	var req groupRequest
	if err := c.BodyParser(&req); err != nil {
		return apierr.BadRequest("invalid JSON body")
	}
	if err := s.db.UpdateUserGroup(c.Params("id"), ptrIfSet(req.Name), ptrIfSet(req.Description)); err != nil {
		if err == storage.ErrNotFound {
			return apierr.NotFoundf("group not found")
		}
		return apierr.Conflict(err.Error())
	}
	updated, _ := s.db.GetUserGroup(c.Params("id"))
	return resp.OK(c, updated)
}

func (s *Server) handleDeleteGroup(c *fiber.Ctx) error {
	ok, err := s.db.DeleteUserGroup(c.Params("id"))
	if err != nil {
		return apierr.StorageErr(err)
	}
	return resp.OK(c, fiber.Map{"deleted": ok})
}

func (s *Server) handleListGroupMembers(c *fiber.Ctx) error {
	users, err := s.db.ListGroupMembers(c.Params("id"))
	if err != nil {
		return apierr.StorageErr(err)
	}
	return resp.OK(c, users)
}

type addMemberRequest struct {
	UserID string `json:"user_id"`
}

func (s *Server) handleAddGroupMember(c *fiber.Ctx) error {
	var req addMemberRequest
	if err := c.BodyParser(&req); err != nil || req.UserID == "" {
		return apierr.BadRequest("user_id is required")
	}
	if err := s.db.AddGroupMember(c.Params("id"), req.UserID); err != nil {
		return apierr.StorageErr(err)
	}
	return resp.OK(c, fiber.Map{"added": true})
}

func (s *Server) handleRemoveGroupMember(c *fiber.Ctx) error {
	ok, err := s.db.RemoveGroupMember(c.Params("id"), c.Params("userID"))
	if err != nil {
		return apierr.StorageErr(err)
	}
	return resp.OK(c, fiber.Map{"removed": ok})
}

// --- host grants ---

func (s *Server) handleListGrants(c *fiber.Ctx) error {
	grants, err := s.db.ListHostGrants()
	if err != nil {
		return apierr.StorageErr(err)
	}
	return resp.OK(c, grants)
}

type grantRequest struct {
	UserGroupID string `json:"user_group_id"`
	ServerID    string `json:"server_id"`
	GroupName   string `json:"group_name"`
	CanConnect  bool   `json:"can_connect"`
	CanExecute  bool   `json:"can_execute"`
}

func (s *Server) handleCreateGrant(c *fiber.Ctx) error {
	var req grantRequest
	if err := c.BodyParser(&req); err != nil {
		return apierr.BadRequest("invalid JSON body")
	}
	if req.UserGroupID == "" {
		return apierr.Unprocessable("user_group_id is required")
	}
	if (req.ServerID == "") == (req.GroupName == "") {
		return apierr.Unprocessable("exactly one of server_id or group_name is required")
	}
	if _, err := s.db.GetUserGroup(req.UserGroupID); err != nil {
		return apierr.NotFoundf("user group not found")
	}
	if req.ServerID != "" {
		if _, err := s.db.GetServer(req.ServerID); err != nil {
			return apierr.NotFoundf("server not found")
		}
	}
	grant := &domain.HostGrant{
		ID:          storage.GenerateID("grant"),
		UserGroupID: req.UserGroupID,
		ServerID:    req.ServerID,
		GroupName:   req.GroupName,
		CanConnect:  req.CanConnect,
		CanExecute:  req.CanExecute,
		CreatedBy:   identityFrom(c).Username,
	}
	if err := s.db.CreateHostGrant(grant); err != nil {
		return apierr.StorageErr(err)
	}
	s.auditAction(c, "grant.create", "create host grant", domain.DecisionAllow, "")
	return resp.Created(c, grant)
}

type patchGrantRequest struct {
	CanConnect *bool `json:"can_connect"`
	CanExecute *bool `json:"can_execute"`
}

func (s *Server) handlePatchGrant(c *fiber.Ctx) error {
	var req patchGrantRequest
	if err := c.BodyParser(&req); err != nil {
		return apierr.BadRequest("invalid JSON body")
	}
	if err := s.db.UpdateHostGrant(c.Params("id"), req.CanConnect, req.CanExecute); err != nil {
		if err == storage.ErrNotFound {
			return apierr.NotFoundf("grant not found")
		}
		return apierr.StorageErr(err)
	}
	return resp.OK(c, fiber.Map{"updated": true})
}

func (s *Server) handleDeleteGrant(c *fiber.Ctx) error {
	ok, err := s.db.DeleteHostGrant(c.Params("id"))
	if err != nil {
		return apierr.StorageErr(err)
	}
	return resp.OK(c, fiber.Map{"deleted": ok})
}

// --- session administration ---

func (s *Server) handleListAllSessions(c *fiber.Ctx) error {
	sessions, err := s.db.ListAllSessions()
	if err != nil {
		return apierr.StorageErr(err)
	}
	return resp.OK(c, sessions)
}

func (s *Server) handleKickSession(c *fiber.Ctx) error {
	ok, err := s.db.DeleteSession(c.Params("id"))
	if err != nil {
		return apierr.StorageErr(err)
	}
	return resp.OK(c, fiber.Map{"deleted": ok})
}

// --- approvals ---

func (s *Server) handleListApprovals(c *fiber.Ctx) error {
	return resp.OK(c, s.broker.ListPending())
}

type decideRequest struct {
	Approved bool `json:"approved"`
}

func (s *Server) handleDecideApproval(c *fiber.Ctx) error {
	var req decideRequest
	if err := c.BodyParser(&req); err != nil {
		return apierr.BadRequest("invalid JSON body")
	}
	decision, ok := s.broker.Decide(c.Params("id"), req.Approved)
	if !ok {
		return apierr.NotFoundf("approval request not found or already resolved")
	}
	id := identityFrom(c)
	if reqPending, exists := s.broker.GetPending(c.Params("id")); exists {
		s.Audit(&domain.AuditEvent{
			ID: newAuditID(), Actor: "approval", Timestamp: time.Now().UTC(),
			ServerID: reqPending.ServerID, ServerName: reqPending.ServerName,
			Command: "approval.decide", Intent: "decision: " + string(decision),
			Decision: domain.DecisionAllow,
			UserID:   id.UserID, Username: id.Username,
		})
	}
	return resp.OK(c, fiber.Map{"decision": decision})
}

// --- tokens (PAT for MCP) ---

func (s *Server) registerTokenRoutes(g fiber.Router) {
	tg := g.Group("/tokens", requirePerm(domain.PermMCPAccess))
	tg.Get("/", s.handleListTokens)
	tg.Post("/", s.handleCreateToken)
	tg.Delete("/:id", s.handleRevokeToken)
}

func (s *Server) handleListTokens(c *fiber.Ctx) error {
	id := identityFrom(c)
	tokens, err := s.db.ListApiTokensForUser(id.UserID)
	if err != nil {
		return apierr.StorageErr(err)
	}
	return resp.OK(c, tokens)
}

type createTokenRequest struct {
	Name        string `json:"name"`
	ExpiresDays int    `json:"expires_days"` // 0 = no expiry
}

func (s *Server) handleCreateToken(c *fiber.Ctx) error {
	var req createTokenRequest
	if err := c.BodyParser(&req); err != nil {
		return apierr.BadRequest("invalid JSON body")
	}
	if req.Name == "" {
		req.Name = "mcp-token"
	}
	id := identityFrom(c)
	token, meta := auth.NewApiToken()
	meta.ID = storage.GenerateID("tok")
	meta.UserID = id.UserID
	meta.Name = req.Name
	if req.ExpiresDays > 0 {
		exp := time.Now().UTC().AddDate(0, 0, req.ExpiresDays)
		meta.ExpiresAt = &exp
	}
	if err := s.db.CreateApiToken(&meta, auth.HashToken(token)); err != nil {
		return apierr.StorageErr(err)
	}
	s.auditAction(c, "token.create", "issue API token "+meta.Name, domain.DecisionAllow, "")
	// Plaintext token is returned exactly once.
	return resp.Created(c, fiber.Map{"token": token, "meta": meta})
}

func (s *Server) handleRevokeToken(c *fiber.Ctx) error {
	id := identityFrom(c)
	ok, err := s.db.RevokeApiToken(c.Params("id"), id.UserID)
	if err != nil {
		return apierr.StorageErr(err)
	}
	return resp.OK(c, fiber.Map{"revoked": ok})
}

// --- small helpers ---

func ptrIfSet(s string) *string {
	if s == "" {
		return nil
	}
	return &s
}

func newAuditID() string { return storage.GenerateID("aud") }
