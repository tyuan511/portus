# Sshield 产品调研、架构设计与落地规划

> 本文档用于新会话 / 新工作区的上下文交接，完整记录前期市场调研结论、技术选型与分阶段研发计划。

## 1. 市场定位与机会点
- **痛点**：传统堡垒机（JumpServer 等）架构过重且非 AI 原生；开源 SSH MCP Server（`ssh-mcp-server` 等）只有静态正则黑白名单，缺乏可视化管理、AST 防绕过、AI 语义审核与人工审批流。
- **定位**：**轻量版 JumpServer + Hoop.dev 的 AI 审核护栏 + Termius 的连接体验 + 原生 MCP Server**。

## 2. 技术栈选型（Rust 全栈 + React）
| 模块 | 选型 | 说明 |
| :--- | :--- | :--- |
| SSH / 跳板机 | `russh` + `russh-sftp` + `ssh2-config` | 纯 Rust 异步实现，通过 `channel_open_direct_tcpip` 链式嵌套实现多级 `ProxyJump` |
| MCP 协议 | `rmcp` (官方 Rust SDK) | `stdio`（本地桌面端）+ `Streamable HTTP`（团队服务端） |
| Shell AST 解析 | `brush-parser` | 将命令解析为 AST，拆解 `|`、`&&`、`$()`，解析失败遵循 Fail-Closed 原则 |
| Web 服务端 | `axum` + `tokio` + `rust-embed` | 单二进制交付，WebSocket 推送终端流与实时审批 |
| 桌面客户端 | `Tauri v2` + `keyring` | 复用 React 前端，本地 SQLite + 系统密钥链 + 原生审批弹窗 |
| 存储层 | `sqlx` | 同一套逻辑支持 SQLite（单机）与 PostgreSQL（团队） |
| 前端 | React + Vite + Tailwind + `xterm.js` | 通过 Transport Adapter 同时适配 Tauri IPC 与 Server HTTP/WS |

## 3. 分阶段实施路线图

### Phase 1: MVP（单兵闭环）
1. `sshield-ssh`：实现 `ssh2-config` 导入 + `russh` 连接池与 `ProxyJump` 跳板。
2. `sshield-security`：基于 `brush-parser` 实现 L1 AST 命令拆解与黑白名单校验。
3. `sshield-mcp`：基于 `rmcp` 暴露 `list_authorized_servers`、`execute_command`（含 `intent`、`dry_run`、`timeout`）。
4. `apps/web` + `apps/desktop`：完成资产管理、规则配置、本地人工确认弹窗与 JSON 审计日志。

### Phase 2: AI 审核与防泄露
1. 接入 L2 AI 语义审核（意图一致性检查、风险评分与自然语言解释）。
2. 流式输出脱敏（Output Masking），防止 `.env`/密码/AKSK 泄露给外部大模型。
3. 审批超时异步状态机（`PENDING_APPROVAL` + `check_approval_status`）。

### Phase 3: 团队服务端与企业能力
1. `apps/server` 完善 RBAC、多 Agent Token 隔离与配额限制。
2. 飞书 / 钉钉 / 企业微信 / Slack 审批卡片集成。
3. 预设只读诊断 MCP 工具集（Docker / K8s / Systemd / Nginx）。
