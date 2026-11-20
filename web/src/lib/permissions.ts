import type { Permission } from "@/types";

// Mirrors domain.AllPermissions() order.
export const ALL_PERMISSIONS: Permission[] = [
  "host.read", "host.connect", "host.execute", "host.manage",
  "vault.manage", "rules.read", "rules.manage",
  "snippets.use", "snippets.manage",
  "audit.view_self", "audit.view_all", "approvals.decide", "mcp.access",
  "users.manage", "roles.manage", "groups.manage", "access.manage", "settings.manage",
];
