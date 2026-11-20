import { createContext, useContext, useEffect, useMemo, useState } from "react";
import type { ReactNode } from "react";
import type { Permission } from "@/types";

export type Lang = "en" | "zh-CN";

const LANG_KEY = "sshield.lang";

// Human-readable names for every permission key, shown in the role matrix.
export const PERMISSION_LABELS: Record<Permission, Record<Lang, string>> = {
  "host.read": { en: "View hosts", "zh-CN": "查看主机" },
  "host.connect": { en: "Connect to hosts", "zh-CN": "连接主机" },
  "host.execute": { en: "Execute commands", "zh-CN": "执行命令" },
  "host.manage": { en: "Manage hosts", "zh-CN": "管理主机" },
  "vault.manage": { en: "Manage credential vault", "zh-CN": "管理凭据库" },
  "rules.read": { en: "View security rules", "zh-CN": "查看安全规则" },
  "rules.manage": { en: "Manage security rules", "zh-CN": "管理安全规则" },
  "snippets.use": { en: "Use snippets", "zh-CN": "使用片段" },
  "snippets.manage": { en: "Manage snippets", "zh-CN": "管理片段" },
  "audit.view_self": { en: "View own audit logs", "zh-CN": "查看本人日志" },
  "audit.view_all": { en: "View all audit logs", "zh-CN": "查看全部日志" },
  "approvals.decide": { en: "Decide approvals", "zh-CN": "审批决策" },
  "mcp.access": { en: "Access via MCP", "zh-CN": "MCP 接入" },
  "users.manage": { en: "Manage users", "zh-CN": "管理用户" },
  "roles.manage": { en: "Manage roles", "zh-CN": "管理角色" },
  "groups.manage": { en: "Manage user groups", "zh-CN": "管理用户组" },
  "access.manage": { en: "Manage host grants", "zh-CN": "管理主机授权" },
  "settings.manage": { en: "Manage global settings", "zh-CN": "管理全局设置" },
};

// UI strings. en is the canonical key; zh-CN mirrors it.
const messages = {
  en: {
    "nav.hosts": "Hosts",
    "nav.approvals": "Approvals",
    "nav.audit": "Audit Logs",
    "nav.security": "Security",
    "nav.admin": "Admin",
    "nav.settings": "Settings",
    "nav.signout": "Sign out",
    "common.refresh": "Refresh",
    "common.save": "Save",
    "common.cancel": "Cancel",
    "common.delete": "Delete",
    "common.create": "Create",
    "common.copy": "Copy",
    "common.search": "Search…",
    "common.enabled": "Enabled",
    "common.loading": "Loading SSHield…",
    "landing.hero.title1": "Ship commands to production",
    "landing.hero.title2": "with a safety net.",
    "landing.hero.body":
      "SSHield sits between your team, your AI agents and your servers: RBAC-scoped access, a layered command security pipeline, and a tamper-evident audit trail — in one binary with an embedded console.",
    "landing.cta.console": "Open console",
    "landing.cta.signin": "Sign in",
    "landing.cta.pipeline": "How it guards commands",
    "landing.badge": "SSHield Enterprise — multi-user SSH gateway",
    "landing.pipeline.title": "Four layers between intent and impact",
    "landing.pipeline.sub": "Commands from the console and from AI agents pass the same pipeline — and every layer is recorded.",
    "landing.features.title": "Enterprise access control, developer-grade UX",
    "login.title": "Sign in",
    "login.subtitle": "to your SSHield operations console",
    "login.username": "Username",
    "login.password": "Password",
    "login.totp": "Authenticator code",
    "login.back": "Back to home",
    "login.provisioned": "Accounts are provisioned by your administrator.",
    "login.overview": "Back to overview",
    "login.signingin": "Signing in…",
    "login.submit": "Sign in",
    "login.totpPrompt": "Enter your 6-digit authenticator code",
    "settings.title": "Settings",
    "settings.subtitle": "Appearance, language, API tokens, two-factor, active sessions",
    "settings.appearance": "Appearance",
    "settings.language": "Language",
    "settings.tokens": "API Tokens",
    "settings.totp": "Two-Factor",
    "settings.sessions": "Sessions",
    "hosts.title": "Hosts",
    "hosts.subtitle": "Server inventory and access targets",
    "hosts.add": "Add host",
    "hosts.import": "Import ssh config",
    "hosts.importTitle": "Import OpenSSH config",
    "hosts.importPaste": "Paste your ~/.ssh/config content",
    "hosts.importPick": "Choose a file from this device",
    "hosts.importHint": "…or pick a config file from this device",
    "hosts.connect": "Connect",
    "hosts.test": "Test",
    "hosts.empty": "No hosts granted to your account.",
    "hosts.emptyAdmin": "No hosts yet — add your first server",
    "admin.title": "Administration",
    "admin.subtitle": "Users, roles, groups, host access and live sessions",
    "admin.users": "Users",
    "admin.roles": "Roles",
    "admin.groups": "Groups",
    "admin.access": "Host Access",
    "admin.sessions": "Sessions",
    "admin.users.add": "Add user",
    "admin.roles.add": "New role",
    "admin.roles.subtitle": "The admin role is immutable. Toggle permissions per role — changes take effect after the user signs in again.",
    "audit.title": "Audit Logs",
    "audit.subtitle.all": "All users' operations",
    "audit.subtitle.self": "Your own and MCP activity",
    "audit.verify": "Verify chain",
    "security.title": "Security",
    "security.subtitle": "L1 command rules, L2 semantic review, snippets and the evaluation sandbox",
    "security.rules": "L1 Rules",
    "security.l2": "L2 Reviewer",
    "security.sandbox": "Sandbox",
    "security.snippets": "Snippets",
  },
  "zh-CN": {
    "nav.hosts": "主机",
    "nav.approvals": "审批",
    "nav.audit": "审计日志",
    "nav.security": "安全",
    "nav.admin": "管理",
    "nav.settings": "设置",
    "nav.signout": "退出登录",
    "common.refresh": "刷新",
    "common.save": "保存",
    "common.cancel": "取消",
    "common.delete": "删除",
    "common.create": "创建",
    "common.copy": "复制",
    "common.search": "搜索…",
    "common.enabled": "启用",
    "common.loading": "正在加载 SSHield…",
    "landing.hero.title1": "让命令安全抵达生产环境",
    "landing.hero.title2": "每一步都有防护。",
    "landing.hero.body":
      "SSHield 位于团队、AI 代理与服务器之间：RBAC 精细化授权、分层命令安全流水线、防篡改审计链 —— 全部封装在一个内置控制台的单二进制里。",
    "landing.cta.console": "进入控制台",
    "landing.cta.signin": "登录",
    "landing.cta.pipeline": "了解命令防护机制",
    "landing.badge": "SSHield 企业版 — 多用户 SSH 网关",
    "landing.pipeline.title": "从意图到影响之间的四道防线",
    "landing.pipeline.sub": "控制台与 AI 代理的命令走同一条流水线 —— 每一层都会留痕。",
    "landing.features.title": "企业级访问控制，开发者级体验",
    "login.title": "登录",
    "login.subtitle": "进入你的 SSHield 运维控制台",
    "login.username": "用户名",
    "login.password": "密码",
    "login.totp": "动态验证码",
    "login.back": "返回首页",
    "login.provisioned": "账号由管理员统一开通。",
    "login.overview": "返回概览",
    "login.signingin": "正在登录…",
    "login.submit": "登录",
    "login.totpPrompt": "请输入 6 位动态验证码",
    "settings.title": "设置",
    "settings.subtitle": "外观、语言、API 令牌、两步验证、活动会话",
    "settings.appearance": "外观",
    "settings.language": "语言",
    "settings.tokens": "API 令牌",
    "settings.totp": "两步验证",
    "settings.sessions": "活动会话",
    "hosts.title": "主机",
    "hosts.subtitle": "服务器清单与访问目标",
    "hosts.add": "添加主机",
    "hosts.import": "导入 ssh 配置",
    "hosts.importTitle": "导入 OpenSSH 配置",
    "hosts.importPaste": "粘贴 ~/.ssh/config 内容",
    "hosts.importPick": "从本机选择配置文件",
    "hosts.importHint": "…或直接从本机选择一个配置文件",
    "hosts.connect": "连接",
    "hosts.test": "测试",
    "hosts.empty": "尚未有任何授权给你的主机。",
    "hosts.emptyAdmin": "还没有主机 —— 添加第一台服务器",
    "admin.title": "管理中心",
    "admin.subtitle": "用户、角色、用户组、主机授权与活动会话",
    "admin.users": "用户",
    "admin.roles": "角色",
    "admin.groups": "用户组",
    "admin.access": "主机授权",
    "admin.sessions": "会话",
    "admin.users.add": "添加用户",
    "admin.roles.add": "新建角色",
    "admin.roles.subtitle": "admin 角色不可修改。按角色勾选权限 —— 用户重新登录后生效。",
    "audit.title": "审计日志",
    "audit.subtitle.all": "全部用户的操作记录",
    "audit.subtitle.self": "你本人与 MCP 的操作记录",
    "audit.verify": "校验审计链",
    "security.title": "安全",
    "security.subtitle": "L1 命令规则、L2 语义审查、命令片段与评测沙盒",
    "security.rules": "L1 规则",
    "security.l2": "L2 审查",
    "security.sandbox": "沙盒",
    "security.snippets": "命令片段",
  },
} as const;

export type MessageKey = keyof (typeof messages)["en"];

interface I18nState {
  lang: Lang;
  setLang: (l: Lang) => void;
  t: (key: MessageKey) => string;
}

const I18nContext = createContext<I18nState | null>(null);

function detectLang(): Lang {
  try {
    const saved = localStorage.getItem(LANG_KEY);
    if (saved === "en" || saved === "zh-CN") return saved;
  } catch {
    // ignore
  }
  return navigator.language.startsWith("zh") ? "zh-CN" : "en";
}

export function LanguageProvider({ children }: { children: ReactNode }) {
  const [lang, setLangState] = useState<Lang>(detectLang);

  useEffect(() => {
    document.documentElement.lang = lang;
  }, [lang]);

  const value = useMemo<I18nState>(
    () => ({
      lang,
      setLang: (l) => {
        setLangState(l);
        try {
          localStorage.setItem(LANG_KEY, l);
        } catch {
          // ignore
        }
      },
      t: (key) => messages[lang][key] ?? messages.en[key] ?? key,
    }),
    [lang]
  );

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n(): I18nState {
  const ctx = useContext(I18nContext);
  if (!ctx) throw new Error("useI18n outside LanguageProvider");
  return ctx;
}

// permissionLabel returns the localized name of a permission key,
// falling back to the raw key.
export function permissionLabel(lang: Lang, perm: Permission): string {
  return PERMISSION_LABELS[perm]?.[lang] ?? perm;
}
