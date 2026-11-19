# Sshield

**AI 时代的轻量级安全运维网关与堡垒机（Rust 全栈）**

> *SSH + Shield* —— 守在 AI Agent 与生产服务器之间的安全护盾。

## 核心特性
- **多服务器与跳板机管理**：无侵入（Agentless）SSH/SFTP 连接，支持 `~/.ssh/config` 导入与多级 `ProxyJump` 跳板链路。
- **双层命令安全引擎**：
  - **L1 确定性规则引擎**：基于 Shell AST 语法树解析（`brush-parser`），防止管道/子命令绕过，严格执行分级黑白名单。
  - **L2 AI 智能语义审核**：意图-命令一致性校验、风险评分与自然语言解读、返回结果敏感信息自动脱敏（Masking）。
- **人机协同审批（HITL）**：高危命令自动挂起，支持桌面弹窗 / Web / IM 交互式审批。
- **原生 MCP Server**：支持 `stdio`（本地客户端）与 `Streamable HTTP`（团队服务端），无缝接入 Cursor / Claude / 自研 Agent。
- **双端形态**：
  - **单人桌面版**：`Tauri v2` + `React`（本地 SQLite + 系统密钥链 + 原生审批弹窗）
  - **团队服务端**：`Axum` + `React`（单二进制部署 + PostgreSQL + RBAC + 统一审计）

## 目录结构
- `crates/sshield-core`：核心领域模型与策略抽象
- `crates/sshield-ssh`：纯 Rust SSH/SFTP 与多级跳板机隧道（`russh`）
- `crates/sshield-security`：Shell AST 规则引擎、AI 审核与输出脱敏
- `crates/sshield-mcp`：原生 MCP Server（`rmcp`）
- `crates/sshield-storage`：SQLite / PostgreSQL 持久层（`sqlx`）
- `apps/server`：团队版服务端（`axum`）
- `apps/desktop`（待初始化）：单人版客户端（`Tauri v2`）
- `apps/web`（待初始化）：React + TypeScript 前端工作台

详细调研与产品规划见 [docs/ARCHITECTURE_AND_PLAN.md](docs/ARCHITECTURE_AND_PLAN.md)。
