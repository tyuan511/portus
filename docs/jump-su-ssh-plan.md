# 跳板机 `su` + `ssh`（路径 B）实现方案

## 1. 需求背景

部分生产环境存在如下访问链路要求：
1. 先通过 SSH 登录跳板机；
2. 在跳板机上执行 `su - <目标账号>` 并输入密码切换身份；
3. 以切换后的账号身份执行 `ssh` 连接到目标服务器（目标机认证可能依赖该账号在跳板机本地的 SSH 私钥，也可能使用 Sshield Vault 中托管的目标机密码）。

该链路需要同时支持以下三类核心操作：
- **Web 交互式终端**（含终端录像与 L1/L2 命令审计）
- **SFTP 文件管理**（目录浏览、上传、下载、重命名、删除）
- **命令执行**（MCP / API `ExecuteCommand` 及主机连通性测试 `TestConnection`）

---

## 2. 为什么现有 `ProxyJump` 不适用

当前 [internal/sshpool/pool.go](../internal/sshpool/pool.go) 中的 `dialChain` 使用标准 SSH `direct-tcpip` 端口转发（`client.Dial("tcp", addr)`），仅在跳板机 `sshd` 层转发原始 TCP 流量，不会启动跳板机 Shell，因此无法执行 `su` 切换用户与交互式密码输入。

---

## 3. 总体架构（路径 B：跳板机 Shell `su` + `ssh`）

### 3.1 核心流程

1. **连接跳板机**：通过现有 `Pool.GetOrConnect(jumpHost, all)` 建立/复用跳板机 `*gossh.Client`（支持跳板机自身再挂前置 `direct_tcpip` 跳板机）。
2. **开启跳板机 PTY 会话并执行 `su`**：
   - 若配置了 `jump_su_username`：
     - 先发送同步标记（如 `echo __SSHIELD_BOOT__`）排空 MOTD/登录横幅；
     - 执行 `LC_ALL=C su - <su_username>`；
     - 在 Expect 状态机中匹配密码提示（`password:` / `密码：` / `passphrase`），从 Vault 读取 `jump_su_credential_id` 对应的密码并注入；
     - 识别认证失败提示（`authentication failure` / `incorrect password` / `sorry`）并立即返回明确错误；
     - 发送 `echo __SSHIELD_SU_OK__` 验证已成功进入目标账号的 Shell。
   - 若 `jump_su_username` 为空：跳过 `su`，直接在跳板机当前账号 Shell 中执行后续 `ssh`（兼容禁用了 `AllowTcpForwarding` 的跳板机）。
3. **根据业务场景分流承载**：

| 能力 | 跳板机执行的命令 | 数据通道处理 |
| :--- | :--- | :--- |
| **Web 交互终端** (`OpenTerminal`) | `exec ssh -tt -o StrictHostKeyChecking=no -p <port> <user>@<host>` | 保留 PTY；若目标机使用 `keyring_password`，在 Expect 阶段自动匹配 `password:` 并注入目标机密码，随后将 `stdin`/`stdout` 接入 `TerminalSession`（`window-change` 自动透传） |
| **命令执行** (`ExecuteCommand`) | `stty raw -echo` → 同步标记 → `exec ssh -T -o StrictHostKeyChecking=no -p <port> <user>@<host> <quoted_cmd>` | 关闭回显与换行转换；若目标机需密码则自动注入，随后读取原始输出与 `session.Wait()` 退出码 |
| **SFTP 文件管理** (`OpenSFTP`) | `stty raw -echo` → 同步标记 → `exec ssh -o StrictHostKeyChecking=no -p <port> -s <user>@<host> sftp` | 若目标机需密码则先注入密码；建立二进制流后通过 `sftp.NewClientPipe(stdout, stdin)` 封装为标准 `*sftp.Client` |
| **连通性测试** (`TestConnection`) | 复用 `ExecuteCommand(..., "true", ...)` | 端到端验证「跳板机 → `su` → `ssh` → 目标机」完整链路 |

---

## 4. 模块改造清单

### 4.1 数据模型与持久化（已完成）

- **Domain 模型**（[internal/domain/asset.go](../internal/domain/asset.go)）：
  - 新增 `JumpMode` 类型：`direct_tcpip`（默认）与 `su_ssh`。
  - `ServerAsset` 新增字段：`JumpMode`、`JumpSuUsername`、`JumpSuCredentialID`。
  - `AuthorizedServerInfo` 暴露 `JumpMode`（不暴露凭据 ID）。
- **数据库迁移**（[internal/storage/migrations/0007_jump_su.sql](../internal/storage/migrations/0007_jump_su.sql)）：
  - 为 `servers` 表增加 `jump_mode`、`jump_su_username`、`jump_su_credential_id` 三列。
- **sqlc 与存储层**（[internal/storage/queries/servers.sql](../internal/storage/queries/servers.sql)、[internal/storage/servers.go](../internal/storage/servers.go)）：
  - 更新 `UpsertServer` SQL 及 `serverToDomain` / `serverParams` 映射，并重新生成 `sqlc` 代码。
- **API 校验**（[internal/httpserver/handlers_servers.go](../internal/httpserver/handlers_servers.go)）：
  - 当 `jump_mode == "su_ssh"` 时校验 `jump_host_id` 不能为空。

### 4.2 SSH 连接池与 `su` + `ssh` 引擎（待实现）

- 新增 `internal/sshpool/jump_su.go`：
  - `bootstrapSuShell`：PTY 打开、MOTD 排空、`su` 执行与密码 Expect、Shell 就绪确认。
  - `enterRawMode`：`stty raw -echo` 与同步标记排空，确保后续字节流纯净。
  - `buildTargetSSHCommand`：安全转义端口、用户、主机及远程命令（`shellQuote`）。
  - `openSuTerminal` / `execSuCommand` / `openSuSFTP`：分别对接 `OpenTerminal`、`ExecuteCommand`、`OpenSFTP`。
- 更新 `internal/sshpool/pool.go`、`terminal.go`、`sftp.go`：
  - 当 `asset.JumpMode == domain.JumpModeSuSSH` 时路由到 `su_ssh` 链路。

### 4.3 前端主机管理 UI（待实现）

- [web/src/types.ts](../web/src/types.ts)：在 `ServerAsset` 接口中补充 `jump_mode`、`jump_su_username`、`jump_su_credential_id`。
- [web/src/pages/Hosts.tsx](../web/src/pages/Hosts.tsx)：
  - 选择跳板机后显示「跳板模式」（标准 ProxyJump / 跳板机 `su` + `ssh`）；
  - 选择 `su_ssh` 模式时显示 `su` 目标账号输入框与 `su` 密码凭据选择器（复用现有 Vault 凭据列表及快捷新建）。
- [web/src/lib/i18n.tsx](../web/src/lib/i18n.tsx)：补充中英文文案。

---

## 5. 测试与验证计划

1. **单元测试**（`internal/sshpool/jump_su_test.go`）：
   - 提示符/密码匹配逻辑、`shellQuote` 安全转义、认证失败检测、基于内存管道或 mock SSH server 的 `su` Expect 状态机测试。
2. **回归测试与构建**：
   - `go test ./...`、`go vet ./...`
   - `cd web && pnpm build`
3. **前提条件与边界**：
   - 跳板机需具备 `su` 与 `ssh` 客户端；
   - 若目标机使用跳板机本地免密私钥（无需交互输入目标机密码），所有三类能力均可直接无感工作；若目标机仍需密码，交互终端与命令/SFTP 均会尝试在 PTY 阶段注入 Vault 中的目标机密码。
