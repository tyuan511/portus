# 16 MiB 终端输出 A/B

2026-11-26，Apple M2，macOS 27.2.1。A 使用 crates.io 的 `alacritty_terminal 0.26.0`。B 使用同版本的本地补丁，只改变网格行回收和调整大小时的脏区域记录。两版使用相同测试程序、依赖版本、Rust nightly 1.102.0（6c709eb42）和 release 配置（opt-level 2）。

## 网格解析

120 列 × 40 行。源负载有 8,388,608 行 `X\n`，共 16 MiB。PTY 启用 ONLCR 后传给解析器的是 24 MiB 的 `X\r\n`。内存回放使用同样的 24 MiB，每次新建网格，计时仅覆盖 `Grid::feed`。每次验证光标、末屏内容、10,000 行回滚深度及最早一行。按 ABBA 运行，每次轮换分块顺序，每种大小每版 10 个样本。

| 每次 feed 的字节数 | A 中位数 | B 中位数 | 耗时下降 |
|---:|---:|---:|---:|
| 3 | 1071.3 ms | 621.1 ms | 42.0% |
| 96 | 660.2 ms | 367.3 ms | 44.4% |
| 4,096 | 533.7 ms | 421.7 ms | 21.0% |
| 65,536 | 489.0 ms | 408.7 ms | 16.4% |

## 真实 SSH PTY

两个只监听回环地址的 OpenSSH 服务，分别测试直连和单跳。按 ABBAAB 运行，每版每条路径 3 次，每个场景使用新会话。逐行场景运行原命令 `yes X | head -c 16777216`，块写场景由 Python 用 64 KiB 块输出相同内容；块写计时包含 Python 启动。两种场景都等就绪后才释放输出，校验接收了全部 8,388,608 个 X，直到完成标记出现在终端网格才停止计时。计时终点在网格，未包含窗口合成。

| 路径、写法 | A 三次耗时（秒） | B 三次耗时（秒） | A 中位数 | B 中位数 |
|---|---|---|---:|---:|
| 直连，原命令 | 15.203 / 16.062 / 16.138 | 13.644 / 14.776 / 12.598 | 16.062 s | 13.644 s |
| 单跳，原命令 | 16.757 / 17.526 / 16.036 | 15.343 / 13.444 / 16.042 | 16.757 s | 15.343 s |
| 直连，64 KiB | 1.208 / 0.484 / 10.081 | 0.891 / 0.616 / 0.388 | 1.208 s | 0.616 s |
| 单跳，64 KiB | 1.328 / 1.075 / 0.782 | 0.751 / 0.600 / 1.488 | 1.075 s | 0.751 s |

本机直接用原生读循环排空 PTY，执行相同的逐行命令，绕过 SSH 和网格。三次耗时为 11.258、12.171、11.229 秒，每轮调用 read 约 400 万至 443 万次。旧轮次的直连和单跳中位数分别为 10.025、9.297 秒。工作站负载有明显波动，本轮原命令未低于旧成绩。块写输入和内存回放用于测量客户端处理能力。

| 其他端到端场景（中位数） | 直连 A / B | 单跳 A / B |
|---|---:|---:|
| 建立会话到 shell 就绪 | 303.703 / 310.078 ms | 371.404 / 241.681 ms |
| 8 MiB 粘贴到消费完毕 | 1516.815 / 1329.477 ms | 1430.700 / 1152.402 ms |
| 单字符到回显钩子 | 0.270 / 0.145 ms | 0.263 / 0.306 ms |

粘贴场景由远端消费满 8 MiB 后才发完成标记。此前的 `dd count=128` 可能在 PTY 短读后提前完成，两种计时口径不同。

## 复测

在 Portus 根目录运行。网格回放可以独立执行。SSH 测试需要本机 sshd 的端口及其认可的身份文件，单跳还需要允许 TCP 转发的第二个 sshd。远端需有 Python 3 和支持 `head -c` 的 shell 环境。

```sh
export CARGO_PROFILE_RELEASE_OPT_LEVEL=2
cargo test --release --locked benchmark_grid_output_fragments -- --ignored --nocapture

export PORTUS_BENCH_RUSSH_PORT="$TARGET_PORT"
export PORTUS_BENCH_RUSSH_JUMP_PORT="$JUMP_PORT"
export PORTUS_BENCH_RUSSH_IDENTITY="$TEST_IDENTITY"
export PORTUS_BENCH_RUSSH_SAMPLES=3
cargo test --release --locked benchmark_russh_end_to_end_pty_when_enabled -- --ignored --nocapture
```

可用 `PORTUS_BENCH_RUSSH_USER` 指定 sshd 登录用户，默认取 `USER`。复现 A 时，在独立副本里把 `vendor/gpui-terminal/Cargo.toml` 的 `alacritty_terminal` 依赖改为 `"=0.26.0"`，让 Cargo 更新这项锁文件来源后用相同配置构建。B 使用仓库内的路径依赖。Alacritty 源码对应上游提交 `94e7c8874e526b1e67b349d9ba30ddf81669119e`，本地保留了 Apache 2.0 许可证。
