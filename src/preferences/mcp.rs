use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{
    AnyElement, App, AppContext as _, ClipboardItem, Context, ElementId, Entity, FontWeight,
    InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window,
    component::{
        IndexPath, Sizable as _,
        button::{Button, ButtonVariants as _},
        input::InputState,
        select::{Select, SelectEvent, SelectState},
    },
    div, prelude::FluentBuilder as _, px, rgb, transparent_black,
};

use crate::icons::Icon;
use crate::mcp::{
    AuditVerdict, MCP_TOOLS, McpAuditSession, McpRuntime, McpSnapshot, McpToolRisk,
    build_client_config, build_connect_prompt,
};
use crate::settings::{
    HostProtocol, Language, MIN_MCP_PORT, McpGuardAction, McpGuardRule, McpHostGrant, McpTransport,
    SettingsStore,
};
use crate::theme::Colors;

use super::{SelectChoice, danger_button, input_field, primary_button, secondary_button, toggle_switch};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum McpSubTab {
    #[default]
    Connect,
    Grants,
    Sessions,
    Audit,
}

const CLIENT_CHOICES: [(&str, &str); 6] = [
    ("claude", "Claude Code"),
    ("codex", "Codex"),
    ("antigravity", "Antigravity"),
    ("cursor", "Cursor"),
    ("opencode", "OpenCode"),
    ("pi", "Pi"),
];

pub struct McpPage {
    store: Entity<SettingsStore>,
    runtime: Arc<McpRuntime>,
    sub_tab: McpSubTab,
    client_id: String,
    prompt_lang: Language,
    client_select: Entity<SelectState<Vec<SelectChoice<String>>>>,
    ttl_select: Entity<SelectState<Vec<SelectChoice<u16>>>>,
    port_input: Entity<InputState>,
    guard_pattern_input: Entity<InputState>,
    replay_session_id: Option<String>,
    replay_step: usize,
    replay_playing: bool,
    replay_gen: u64,
    _subs: Vec<Subscription>,
}

impl McpPage {
    pub fn new(
        store: Entity<SettingsStore>,
        runtime: Arc<McpRuntime>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (language, port, ttl) = {
            let current = &store.read(cx).current;
            (current.language, current.mcp.port, current.mcp.session_ttl_mins)
        };

        let client_items: Vec<SelectChoice<String>> = CLIENT_CHOICES
            .iter()
            .map(|(id, label)| SelectChoice::new(*label, (*id).to_owned()))
            .collect();
        let client_select = cx.new(|cx| {
            SelectState::new(client_items, Some(IndexPath::default().row(0)), window, cx)
        });

        let ttl_items = vec![
            SelectChoice::new(language.text("15 min", "15 分钟"), 15u16),
            SelectChoice::new(language.text("30 min", "30 分钟"), 30u16),
            SelectChoice::new(language.text("1 hour", "1 小时"), 60u16),
        ];
        let ttl_row = match ttl {
            15 => 0,
            60 => 2,
            _ => 1,
        };
        let ttl_select = cx.new(|cx| {
            SelectState::new(ttl_items, Some(IndexPath::default().row(ttl_row)), window, cx)
        });

        let port_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("7331")
                .default_value(port.to_string())
        });
        let guard_pattern_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(language.text(
                "Add high-risk regex, e.g. kubectl delete|helm uninstall",
                "新增高危命令正则，例如: kubectl delete|helm uninstall",
            ))
        });

        let mut subs = Vec::new();
        subs.push(cx.observe(&store, |_, _, cx| cx.notify()));
        subs.push(
            cx.subscribe(&client_select, |this, _, event, cx| {
                if let SelectEvent::Confirm(Some(id)) = event {
                    this.client_id = id.clone();
                    cx.notify();
                }
            }),
        );
        subs.push(
            cx.subscribe_in(&ttl_select, window, |this, _, event, window, cx| {
                if let SelectEvent::Confirm(Some(mins)) = event {
                    let mins = *mins;
                    let res = this.store.update(cx, |store, cx| {
                        let r = store.update(|s| s.mcp.session_ttl_mins = mins);
                        cx.notify();
                        r
                    });
                    if let Err(err) = res {
                        crate::notify::error(err, window, cx);
                    } else {
                        let lang = this.store.read(cx).current.language;
                        crate::notify::success(
                            lang.text("MCP session TTL updated.", "MCP 会话 TTL 已更新"),
                            window,
                            cx,
                        );
                    }
                }
            }),
        );

        Self {
            store,
            runtime,
            sub_tab: McpSubTab::Connect,
            client_id: "claude".to_owned(),
            prompt_lang: language,
            client_select,
            ttl_select,
            port_input,
            guard_pattern_input,
            replay_session_id: None,
            replay_step: 0,
            replay_playing: false,
            replay_gen: 0,
            _subs: subs,
        }
    }

    #[allow(dead_code)]
    pub fn open_sessions_tab(&mut self, cx: &mut Context<Self>) {
        self.sub_tab = McpSubTab::Sessions;
        self.stop_replay();
        cx.notify();
    }

    fn stop_replay(&mut self) {
        self.replay_playing = false;
        self.replay_gen = self.replay_gen.wrapping_add(1);
    }

    fn toggle_server(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let language = self.store.read(cx).current.language;
        let next_enabled = !self.store.read(cx).current.mcp.enabled;
        let res = self.store.update(cx, |store, cx| {
            let r = store.update(|s| s.mcp.enabled = next_enabled);
            cx.notify();
            r
        });
        match res {
            Ok(()) => {
                crate::notify::success(
                    if next_enabled {
                        language.text("Local MCP Server started.", "本地 MCP Server 已启动")
                    } else {
                        language.text("Local MCP Server stopped.", "本地 MCP Server 已停止")
                    },
                    window,
                    cx,
                );
            }
            Err(err) => crate::notify::error(err, window, cx),
        }
    }

    fn set_transport(&mut self, transport: McpTransport, window: &mut Window, cx: &mut Context<Self>) {
        let res = self.store.update(cx, |store, cx| {
            let r = store.update(|s| s.mcp.transport = transport);
            cx.notify();
            r
        });
        if let Err(err) = res {
            crate::notify::error(err, window, cx);
        }
    }

    fn apply_port(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let language = self.store.read(cx).current.language;
        let raw = self.port_input.read(cx).value().trim().to_owned();
        let Ok(port) = raw.parse::<u16>() else {
            crate::notify::error(
                language.text(
                    "Enter a valid port between 1024 and 65535.",
                    "请输入 1024–65535 之间的有效端口。",
                ),
                window,
                cx,
            );
            return;
        };
        if port < MIN_MCP_PORT {
            crate::notify::error(
                language.text(
                    "Enter a valid port between 1024 and 65535.",
                    "请输入 1024–65535 之间的有效端口。",
                ),
                window,
                cx,
            );
            return;
        }
        let res = self.store.update(cx, |store, cx| {
            let r = store.update(|s| s.mcp.port = port);
            cx.notify();
            r
        });
        match res {
            Ok(()) => crate::notify::success(
                format!(
                    "{} 127.0.0.1:{port}",
                    language.text("MCP port updated to", "MCP 本地监听端口已更新为")
                ),
                window,
                cx,
            ),
            Err(err) => crate::notify::error(err, window, cx),
        }
    }

    fn copy_client_config(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (cfg, language) = {
            let cur = &self.store.read(cx).current;
            (build_client_config(&cur.mcp, &self.client_id), cur.language)
        };
        cx.write_to_clipboard(ClipboardItem::new_string(cfg));
        crate::notify::success(
            language.text("MCP client config copied.", "已复制 MCP 客户端接入配置"),
            window,
            cx,
        );
    }

    fn copy_connect_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (prompt, language) = {
            let cur = &self.store.read(cx).current;
            (build_connect_prompt(&cur.mcp, self.prompt_lang), cur.language)
        };
        cx.write_to_clipboard(ClipboardItem::new_string(prompt));
        crate::notify::success(
            language.text(
                "MCP prompt copied — paste it to your AI assistant.",
                "已复制 MCP 接入提示词，可直接粘贴给 AI 助手",
            ),
            window,
            cx,
        );
    }

    fn toggle_tool(&mut self, tool_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let language = self.store.read(cx).current.language;
        let next_enabled = !self.store.read(cx).current.mcp.is_tool_enabled(tool_id);
        let id = tool_id.to_owned();
        let res = self.store.update(cx, |store, cx| {
            let r = store.update(|s| s.mcp.set_tool_enabled(&id, next_enabled));
            cx.notify();
            r
        });
        match res {
            Ok(()) => crate::notify::info(
                format!(
                    "{tool_id} {}",
                    if next_enabled {
                        language.text("enabled", "已启用")
                    } else {
                        language.text("disabled", "已禁用")
                    }
                ),
                window,
                cx,
            ),
            Err(err) => crate::notify::error(err, window, cx),
        }
    }

    fn set_host_grant(
        &mut self,
        host_id: u64,
        grant: McpHostGrant,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let language = self.store.read(cx).current.language;
        let host_name = self
            .store
            .read(cx)
            .current
            .hosts
            .iter()
            .find(|h| h.id == host_id)
            .map(|h| h.name.clone())
            .unwrap_or_else(|| "Host".to_owned());

        let res = self.store.update(cx, |store, cx| {
            let r = store.update(|s| {
                s.mcp.host_grants.insert(host_id, grant);
            });
            cx.notify();
            r
        });
        match res {
            Ok(()) => crate::notify::info(
                format!(
                    "{host_name} MCP: {}",
                    grant.label(language)
                ),
                window,
                cx,
            ),
            Err(err) => crate::notify::error(err, window, cx),
        }
    }

    fn toggle_guard_rule(&mut self, rule_id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let res = self.store.update(cx, |store, cx| {
            let r = store.update(|s| {
                if let Some(rule) = s.mcp.guard_rules.iter_mut().find(|r| r.id == rule_id) {
                    rule.enabled = !rule.enabled;
                }
            });
            cx.notify();
            r
        });
        if let Err(err) = res {
            crate::notify::error(err, window, cx);
        }
    }

    fn add_guard_rule(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let language = self.store.read(cx).current.language;
        let pattern = self.guard_pattern_input.read(cx).value().trim().to_owned();
        if pattern.is_empty() {
            crate::notify::error(
                language.text("Enter a command pattern first.", "请输入高危命令匹配规则"),
                window,
                cx,
            );
            return;
        }
        let label = language.text("Custom Guard Rule", "自定义高危规则").to_owned();
        let res = self.store.update(cx, |store, cx| {
            let r = store.update(|s| {
                let id = s.mcp.next_rule_id();
                s.mcp.guard_rules.push(McpGuardRule {
                    id,
                    pattern,
                    action: McpGuardAction::Approval,
                    label,
                    enabled: true,
                });
            });
            cx.notify();
            r
        });
        match res {
            Ok(()) => {
                self.guard_pattern_input.update(cx, |input, cx| {
                    input.set_value("", window, cx);
                });
                crate::notify::success(
                    language.text("Guard rule added.", "已新增高危命令审批规则"),
                    window,
                    cx,
                );
            }
            Err(err) => crate::notify::error(err, window, cx),
        }
    }

    fn toggle_mask_secrets(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let next = !self.store.read(cx).current.mcp.mask_secrets;
        let res = self.store.update(cx, |store, cx| {
            let r = store.update(|s| s.mcp.mask_secrets = next);
            cx.notify();
            r
        });
        if let Err(err) = res {
            crate::notify::error(err, window, cx);
        }
    }

    fn simulate_approval(&mut self, cx: &mut Context<Self>) {
        let language = self.store.read(cx).current.language;
        self.sub_tab = McpSubTab::Sessions;
        self.stop_replay();
        self.runtime.simulate_approval(language);
        cx.notify();
    }

    fn resolve_approval(
        &mut self,
        id: u64,
        approved: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let language = self.store.read(cx).current.language;
        if let Some(req) = self.runtime.resolve_approval(id, approved) {
            let msg = if approved {
                format!(
                    "{} {} ({})",
                    language.text("Approved command for", "已批准执行："),
                    req.client,
                    req.host
                )
            } else {
                format!(
                    "{} {}",
                    language.text("Denied request from", "已拒绝请求："),
                    req.client
                )
            };
            crate::notify::info(msg, window, cx);
        }
        cx.notify();
    }

    fn kill_session(&mut self, session_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let language = self.store.read(cx).current.language;
        if self.runtime.kill_session(session_id) {
            crate::notify::info(
                format!(
                    "{} {session_id}",
                    language.text("Terminated MCP session", "已强制断开 MCP 会话")
                ),
                window,
                cx,
            );
        }
        cx.notify();
    }

    fn open_replay(&mut self, session_id: String, cx: &mut Context<Self>) {
        self.stop_replay();
        self.replay_session_id = Some(session_id);
        self.replay_step = 0;
        cx.notify();
    }

    fn step_replay(&mut self, delta: isize, total: usize, cx: &mut Context<Self>) {
        self.stop_replay();
        if total == 0 {
            return;
        }
        let next = (self.replay_step as isize + delta).clamp(0, (total - 1) as isize) as usize;
        self.replay_step = next;
        cx.notify();
    }

    fn toggle_replay_play(&mut self, total: usize, window: &mut Window, cx: &mut Context<Self>) {
        if total == 0 {
            return;
        }
        if self.replay_playing {
            self.stop_replay();
            cx.notify();
            return;
        }
        if self.replay_step + 1 >= total {
            self.replay_step = 0;
        }
        self.replay_playing = true;
        self.replay_gen = self.replay_gen.wrapping_add(1);
        let generation = self.replay_gen;
        cx.notify();

        cx.spawn_in(window, async move |page, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(1200))
                    .await;
                let mut keep_going = false;
                let updated = cx.update(|_, cx| {
                    let _ = page.update(cx, |this, cx| {
                        if !this.replay_playing || this.replay_gen != generation {
                            return;
                        }
                        if this.replay_step + 1 < total {
                            this.replay_step += 1;
                            keep_going = true;
                        } else {
                            this.replay_playing = false;
                        }
                        cx.notify();
                    });
                });
                if updated.is_err() || !keep_going {
                    break;
                }
            }
        })
        .detach();
    }
}

impl Render for McpPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = self.store.read(cx).current.clone();
        let colors = settings.theme.colors_for(settings.opacity);
        let language = settings.language;
        let snap = self.runtime.snapshot();
        let pending_count = snap.pending_approvals.len();

        let endpoint = if settings.mcp.transport == McpTransport::Http {
            format!("http://127.0.0.1:{}/mcp", settings.mcp.port)
        } else {
            "portus --mcp-stdio".to_owned()
        };

        let enabled_tools = MCP_TOOLS
            .iter()
            .filter(|t| settings.mcp.is_tool_enabled(t.id))
            .count();

        let replay_session = self
            .replay_session_id
            .as_ref()
            .and_then(|id| snap.audit_sessions.iter().find(|s| &s.id == id))
            .cloned();

        let sessions_tab_label = if pending_count > 0 {
            format!(
                "{} ({pending_count})",
                language.text("Live Sessions & Approvals", "实时会话与审批")
            )
        } else {
            language
                .text("Live Sessions & Approvals", "实时会话与审批")
                .to_owned()
        };

        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(
                // Header
                div()
                    .px(px(22.))
                    .pt(px(16.))
                    .pb(px(12.))
                    .border_b_1()
                    .border_color(colors.hairline())
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .text_size(px(17.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(colors.foreground))
                            .child(language.text("MCP Server", "MCP 服务")),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                secondary_button(
                                    "mcp-sim-btn",
                                    Some(Icon::Plus),
                                    language.text("Simulate Approval", "模拟审批请求"),
                                    colors,
                                )
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.simulate_approval(cx);
                                })),
                            )
                            .child(
                                primary_button(
                                    "mcp-copy-cfg-top",
                                    Some(Icon::Duplicate),
                                    language.text("Copy Client Config", "复制客户端配置"),
                                    colors,
                                )
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.copy_client_config(window, cx);
                                })),
                            ),
                    ),
            )
            .child(
                // Status Banner
                div()
                    .mx(px(22.))
                    .mt(px(14.))
                    .px(px(14.))
                    .py(px(11.))
                    .rounded(px(10.))
                    .bg(colors.surface_glass())
                    .border_1()
                    .border_color(colors.hairline())
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .min_w_0()
                            .child(
                                div()
                                    .size(px(8.))
                                    .rounded_full()
                                    .flex_shrink_0()
                                    .bg(rgb(if snap.running {
                                        colors.success
                                    } else {
                                        colors.muted
                                    })),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(3.))
                                    .min_w_0()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.))
                                            .child(
                                                div()
                                                    .text_size(px(12.5))
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_color(rgb(colors.foreground))
                                                    .child(if snap.running {
                                                        language.text(
                                                            "Local MCP Server Running",
                                                            "本地 MCP Server 运行中",
                                                        )
                                                    } else {
                                                        language.text(
                                                            "Local MCP Server Stopped",
                                                            "本地 MCP Server 已停止",
                                                        )
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .px(px(7.))
                                                    .py(px(2.))
                                                    .rounded(px(5.))
                                                    .bg(colors.content_bg(100))
                                                    .border_1()
                                                    .border_color(colors.hairline())
                                                    .text_size(px(11.))
                                                    .text_color(rgb(colors.secondary))
                                                    .child(endpoint),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(rgb(if snap.bind_error.is_some() {
                                                colors.danger
                                            } else {
                                                colors.secondary
                                            }))
                                            .child(if let Some(err) = &snap.bind_error {
                                                err.clone()
                                            } else if language == Language::English {
                                                format!(
                                                    "{enabled_tools}/{} tools enabled · {} active sessions · {pending_count} pending approvals",
                                                    MCP_TOOLS.len(),
                                                    snap.active_sessions.len()
                                                )
                                            } else {
                                                format!(
                                                    "已启用 {enabled_tools}/{} 个工具 · {} 个活跃会话 · {pending_count} 项待审批",
                                                    MCP_TOOLS.len(),
                                                    snap.active_sessions.len()
                                                )
                                            }),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(
                                div()
                                    .flex()
                                    .p(px(2.))
                                    .rounded(px(7.))
                                    .bg(colors.surface_glass())
                                    .border_1()
                                    .border_color(colors.hairline())
                                    .child(self.seg_btn(
                                        "mcp-tr-http",
                                        "Streamable HTTP",
                                        settings.mcp.transport == McpTransport::Http,
                                        colors,
                                        cx.listener(|this, _, window, cx| {
                                            this.set_transport(McpTransport::Http, window, cx);
                                        }),
                                    ))
                                    .child(self.seg_btn(
                                        "mcp-tr-stdio",
                                        "Stdio",
                                        settings.mcp.transport == McpTransport::Stdio,
                                        colors,
                                        cx.listener(|this, _, window, cx| {
                                            this.set_transport(McpTransport::Stdio, window, cx);
                                        }),
                                    )),
                            )
                            .child(
                                toggle_switch("mcp-server-toggle", settings.mcp.enabled, colors)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.toggle_server(window, cx);
                                    })),
                            ),
                    ),
            )
            .child(
                // Sub-tabs bar
                div()
                    .px(px(22.))
                    .pt(px(12.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex()
                            .p(px(2.))
                            .rounded(px(7.))
                            .bg(colors.surface_glass())
                            .border_1()
                            .border_color(colors.hairline())
                            .child(self.seg_btn(
                                "mcp-tab-connect",
                                language.text("Connect & Tools", "接入与工具"),
                                self.sub_tab == McpSubTab::Connect,
                                colors,
                                cx.listener(|this, _, _, cx| {
                                    this.sub_tab = McpSubTab::Connect;
                                    this.stop_replay();
                                    cx.notify();
                                }),
                            ))
                            .child(self.seg_btn(
                                "mcp-tab-grants",
                                language.text("Host Grants & Guardrails", "主机授权与策略"),
                                self.sub_tab == McpSubTab::Grants,
                                colors,
                                cx.listener(|this, _, _, cx| {
                                    this.sub_tab = McpSubTab::Grants;
                                    this.stop_replay();
                                    cx.notify();
                                }),
                            ))
                            .child(self.seg_btn(
                                "mcp-tab-sessions",
                                sessions_tab_label,
                                self.sub_tab == McpSubTab::Sessions,
                                colors,
                                cx.listener(|this, _, _, cx| {
                                    this.sub_tab = McpSubTab::Sessions;
                                    this.stop_replay();
                                    cx.notify();
                                }),
                            ))
                            .child(self.seg_btn(
                                "mcp-tab-audit",
                                language.text("Audit & Replay", "审计与回放"),
                                self.sub_tab == McpSubTab::Audit,
                                colors,
                                cx.listener(|this, _, _, cx| {
                                    this.sub_tab = McpSubTab::Audit;
                                    cx.notify();
                                }),
                            )),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(colors.muted))
                            .child(language.text(
                                "Localhost-only · Tamper-evident audit chain",
                                "仅监听本机回环地址 · 全程防篡改审计链",
                            )),
                    ),
            )
            .child(
                // Main body
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .overflow_hidden()
                    .child(
                        div()
                            .id("mcp-scroll-area")
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_y_scroll()
                            .px(px(22.))
                            .py(px(16.))
                            .child(match self.sub_tab {
                                McpSubTab::Connect => self.render_connect_tab(&settings, colors, cx),
                                McpSubTab::Grants => self.render_grants_tab(&settings, colors, cx),
                                McpSubTab::Sessions => self.render_sessions_tab(&snap, colors, language, cx),
                                McpSubTab::Audit => self.render_audit_tab(&snap, colors, language, cx),
                            }),
                    ),
            )
            .children(replay_session.filter(|_| self.sub_tab == McpSubTab::Audit).map(
                |sess| self.render_replay_inspector(&sess, colors, language, cx),
            ))
    }
}

impl McpPage {
    fn seg_btn(
        &self,
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        active: bool,
        colors: Colors,
        on_click: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        div()
            .id(id)
            .h(px(24.))
            .px(px(10.))
            .rounded(px(5.))
            .cursor_pointer()
            .flex()
            .items_center()
            .text_size(px(12.))
            .font_weight(if active {
                FontWeight::SEMIBOLD
            } else {
                FontWeight::NORMAL
            })
            .bg(if active {
                colors.surface_active_glass()
            } else {
                transparent_black().into()
            })
            .text_color(rgb(if active {
                colors.foreground
            } else {
                colors.secondary
            }))
            .hover(|s| s.bg(colors.surface_hover_glass()))
            .on_click(on_click)
            .child(label.into())
    }

    fn section_title(&self, title: impl Into<SharedString>, colors: Colors) -> AnyElement {
        div()
            .text_size(px(11.5))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(colors.secondary))
            .child(title.into())
            .into_any_element()
    }

    fn card(&self, colors: Colors) -> gpui_kit::Div {
        div()
            .rounded(px(10.))
            .bg(colors.surface_glass())
            .border_1()
            .border_color(colors.hairline())
            .flex()
            .flex_col()
    }

    fn render_connect_tab(
        &self,
        settings: &crate::settings::Settings,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let language = settings.language;
        let cfg = build_client_config(&settings.mcp, &self.client_id);
        let prompt = build_connect_prompt(&settings.mcp, self.prompt_lang);
        let enabled_count = MCP_TOOLS
            .iter()
            .filter(|t| settings.mcp.is_tool_enabled(t.id))
            .count();

        div()
            .max_w(px(780.))
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(self.section_title(
                language.text("Quick Connect AI Clients", "快速接入 AI 客户端"),
                colors,
            ))
            .child(
                self.card(colors)
                    .child(
                        div()
                            .px(px(14.))
                            .py(px(11.))
                            .border_b_1()
                            .border_color(colors.hairline())
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(12.))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(rgb(colors.foreground))
                                            .child(language.text("Client Configuration", "客户端配置")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(rgb(colors.secondary))
                                            .child(language.text(
                                                "Binds strictly to 127.0.0.1 loopback without API keys",
                                                "仅绑定本机回环地址（127.0.0.1），无需密钥，外部网络无法访问",
                                            )),
                                    ),
                            )
                            .child(div().w(px(160.)).child(Select::new(&self.client_select).small().text_xs())),
                    )
                    .child(
                        div()
                            .px(px(14.))
                            .py(px(12.))
                            .flex()
                            .flex_col()
                            .gap(px(10.))
                            .child(
                                div()
                                    .p(px(11.))
                                    .rounded(px(8.))
                                    .bg(colors.term_bg(100))
                                    .border_1()
                                    .border_color(colors.hairline())
                                    .text_size(px(11.5))
                                    .text_color(rgb(colors.foreground))
                                    .child(cfg),
                            )
                            .child(
                                div()
                                    .flex()
                                    .justify_end()
                                    .child(
                                        secondary_button(
                                            "mcp-copy-cfg-inline",
                                            Some(Icon::Duplicate),
                                            language.text("Copy Config", "复制配置"),
                                            colors,
                                        )
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.copy_client_config(window, cx);
                                        })),
                                    ),
                            ),
                    )
                    .children((settings.mcp.transport == McpTransport::Http).then(|| {
                        div()
                            .px(px(14.))
                            .py(px(11.))
                            .border_t_1()
                            .border_color(colors.hairline())
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(12.))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(rgb(colors.foreground))
                                            .child(language.text("Local Listen Port", "本机监听端口")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(rgb(colors.secondary))
                                            .child(language.text(
                                                "Listens on 127.0.0.1 only; LAN and public connections are rejected",
                                                "固定监听 127.0.0.1（localhost），拒绝来自局域网与公网的连接，免密钥直连",
                                            )),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(rgb(colors.secondary))
                                            .child("127.0.0.1 :"),
                                    )
                                    .child(
                                        div()
                                            .w(px(76.))
                                            .child(input_field(&self.port_input, None, colors)),
                                    )
                                    .child(
                                        secondary_button(
                                            "mcp-apply-port",
                                            None,
                                            language.text("Apply", "应用"),
                                            colors,
                                        )
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.apply_port(window, cx);
                                        })),
                                    ),
                            )
                    })),
            )
            .child(self.section_title(
                language.text(
                    "Connect via Prompt (Send to AI Assistant)",
                    "提示词接入（发给 AI 助手自动配置）",
                ),
                colors,
            ))
            .child(
                self.card(colors)
                    .child(
                        div()
                            .px(px(14.))
                            .py(px(11.))
                            .border_b_1()
                            .border_color(colors.hairline())
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(12.))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(rgb(colors.foreground))
                                            .child(language.text("One-Click Setup Prompt", "一键接入提示词")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(rgb(colors.secondary))
                                            .child(language.text(
                                                "Paste to Claude Code / Cursor / Codex / Antigravity to auto-configure and verify",
                                                "复制下方提示词发送给 AI 助手，让 AI 自动完成 MCP 配置并校验连通性",
                                            )),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .p(px(2.))
                                    .rounded(px(7.))
                                    .bg(colors.surface_glass())
                                    .border_1()
                                    .border_color(colors.hairline())
                                    .child(self.seg_btn(
                                        "mcp-prompt-zh",
                                        "中文",
                                        self.prompt_lang == Language::ChineseSimplified,
                                        colors,
                                        cx.listener(|this, _, _, cx| {
                                            this.prompt_lang = Language::ChineseSimplified;
                                            cx.notify();
                                        }),
                                    ))
                                    .child(self.seg_btn(
                                        "mcp-prompt-en",
                                        "English",
                                        self.prompt_lang == Language::English,
                                        colors,
                                        cx.listener(|this, _, _, cx| {
                                            this.prompt_lang = Language::English;
                                            cx.notify();
                                        }),
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .px(px(14.))
                            .py(px(12.))
                            .flex()
                            .flex_col()
                            .gap(px(10.))
                            .child(
                                div()
                                    .p(px(11.))
                                    .rounded(px(8.))
                                    .bg(colors.term_bg(100))
                                    .border_1()
                                    .border_color(colors.hairline())
                                    .text_size(px(12.))
                                    .text_color(rgb(colors.foreground))
                                    .child(prompt),
                            )
                            .child(
                                div().flex().justify_end().child(
                                    primary_button(
                                        "mcp-copy-prompt",
                                        Some(Icon::Duplicate),
                                        language.text("Copy Setup Prompt", "复制接入提示词"),
                                        colors,
                                    )
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.copy_connect_prompt(window, cx);
                                    })),
                                ),
                            ),
                    ),
            )
            .child(self.section_title(
                language.text(
                    "Agent CLI Hooks (Claude Code / Codex / Shell)",
                    "AI Agent CLI Hooks 集成（Claude Code / Codex / Shell）",
                ),
                colors,
            ))
            .child({
                let hooks_json = crate::agent_bus::build_claude_hooks_snippet();
                let hooks_copy = hooks_json.clone();
                self.card(colors)
                    .child(
                        div()
                            .px(px(14.))
                            .py(px(11.))
                            .border_b_1()
                            .border_color(colors.hairline())
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(colors.foreground))
                                    .child(language.text(
                                        "Portus Agent Control Bus (`Portus ctl`)",
                                        "Portus Agent 控制总线（`Portus ctl`）",
                                    )),
                            )
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(rgb(colors.secondary))
                                    .child(language.text(
                                        "Every local terminal exports $PORTUS_SESSION_ID & $PORTUS_SOCKET. Add these hooks to ~/.claude/settings.json for instant Notification Rings & status updates.",
                                        "每个本地终端都会自动注入 $PORTUS_SESSION_ID 与 $PORTUS_SOCKET。将下方 Hooks 添加到 ~/.claude/settings.json 即可联动通知光环与状态徽标。",
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .px(px(14.))
                            .py(px(12.))
                            .flex()
                            .flex_col()
                            .gap(px(10.))
                            .child(
                                div()
                                    .p(px(11.))
                                    .rounded(px(8.))
                                    .bg(colors.term_bg(100))
                                    .border_1()
                                    .border_color(colors.hairline())
                                    .text_size(px(11.5))
                                    .text_color(rgb(colors.foreground))
                                    .child(hooks_json),
                            )
                            .child(
                                div().flex().justify_end().child(
                                    primary_button(
                                        "mcp-copy-hooks",
                                        Some(Icon::Duplicate),
                                        language.text("Copy Claude Hooks", "复制 Claude Hooks"),
                                        colors,
                                    )
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        let lang = this.store.read(cx).current.language;
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            hooks_copy.clone(),
                                        ));
                                        crate::notify::success(
                                            lang.text(
                                                "Copied Claude Code hooks snippet.",
                                                "已复制 Claude Code Hooks 配置。",
                                            ),
                                            window,
                                            cx,
                                        );
                                    })),
                                ),
                            ),
                    )
            })
            .child(self.section_title(
                format!(
                    "{} ({enabled_count}/{})",
                    language.text("Exposed MCP Tools", "对外暴露的 MCP 工具"),
                    MCP_TOOLS.len()
                ),
                colors,
            ))
            .child(
                self.card(colors)
                    .children(MCP_TOOLS.iter().enumerate().map(|(i, tool)| {
                        let enabled = settings.mcp.is_tool_enabled(tool.id);
                        let (badge_bg, badge_fg) = match tool.risk {
                            McpToolRisk::Low => (colors.success_soft(), colors.success),
                            McpToolRisk::Medium => (colors.warning_soft(), colors.warning()),
                            McpToolRisk::High => (colors.danger_soft(), colors.danger),
                        };
                        let tool_id = tool.id;
                        div()
                            .px(px(14.))
                            .py(px(10.))
                            .when(i > 0, |d| d.border_t_1().border_color(colors.hairline()))
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(12.))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .min_w_0()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(7.))
                                            .child(
                                                div()
                                                    .text_size(px(12.5))
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_color(rgb(colors.foreground))
                                                    .child(tool.id),
                                            )
                                            .child(
                                                div()
                                                    .px(px(7.))
                                                    .py(px(1.5))
                                                    .rounded_full()
                                                    .bg(badge_bg)
                                                    .text_size(px(10.5))
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_color(rgb(badge_fg))
                                                    .child(tool.risk.label(language)),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(rgb(colors.secondary))
                                            .child(tool.desc(language)),
                                    ),
                            )
                            .child(
                                toggle_switch(("mcp-tool-sw", i), enabled, colors).on_click(
                                    cx.listener(move |this, _, window, cx| {
                                        this.toggle_tool(tool_id, window, cx);
                                    }),
                                ),
                            )
                    })),
            )
            .into_any_element()
    }

    fn render_grants_tab(
        &self,
        settings: &crate::settings::Settings,
        colors: Colors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let language = settings.language;
        let ssh_hosts: Vec<_> = settings
            .hosts
            .iter()
            .filter(|h| h.protocol == HostProtocol::Ssh)
            .collect();

        div()
            .max_w(px(780.))
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(self.section_title(
                language.text("Per-Host MCP Authorization Matrix", "单主机 MCP 授权矩阵"),
                colors,
            ))
            .child(
                self.card(colors)
                    .child(
                        div()
                            .px(px(14.))
                            .py(px(8.))
                            .bg(colors.content_bg(100))
                            .border_b_1()
                            .border_color(colors.hairline())
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(colors.muted))
                                    .child(language.text("Host / Group", "主机 / 分组")),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(colors.muted))
                                    .child(language.text("AI Access Level", "AI 访问权限级别")),
                            ),
                    )
                    .when(ssh_hosts.is_empty(), |card| {
                        card.child(
                            div()
                                .px(px(14.))
                                .py(px(14.))
                                .text_size(px(12.))
                                .text_color(rgb(colors.secondary))
                                .child(language.text(
                                    "No SSH hosts configured yet. Add hosts in the Hosts tab first.",
                                    "暂无 SSH 主机，请先在「主机」页面添加。",
                                )),
                        )
                    })
                    .children(ssh_hosts.into_iter().enumerate().map(|(i, host)| {
                        let cur_grant = settings.mcp.host_grant(host.id);
                        let host_id = host.id;
                        div()
                            .px(px(14.))
                            .py(px(10.))
                            .when(i > 0, |d| d.border_t_1().border_color(colors.hairline()))
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(12.))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .min_w_0()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(6.))
                                            .child(
                                                div()
                                                    .text_size(px(13.))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(rgb(colors.foreground))
                                                    .child(host.name.clone()),
                                            )
                                            .child(
                                                div()
                                                    .px(px(5.))
                                                    .py(px(1.))
                                                    .rounded(px(4.))
                                                    .bg(colors.accent_soft())
                                                    .text_size(px(10.))
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_color(rgb(colors.accent))
                                                    .child("SSH"),
                                            )
                                            .when(!host.group.is_empty(), |d| {
                                                d.child(
                                                    div()
                                                        .text_size(px(11.))
                                                        .text_color(rgb(colors.muted))
                                                        .child(host.group.clone()),
                                                )
                                            }),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(rgb(colors.secondary))
                                            .child(host.endpoint()),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .p(px(2.))
                                    .rounded(px(7.))
                                    .bg(colors.surface_glass())
                                    .border_1()
                                    .border_color(colors.hairline())
                                    .children(McpHostGrant::ALL.into_iter().map(|g| {
                                        self.seg_btn(
                                            ("mcp-grant", (host_id as usize) * 10 + g as usize),
                                            g.label(language),
                                            cur_grant == g,
                                            colors,
                                            cx.listener(move |this, _, window, cx| {
                                                this.set_host_grant(host_id, g, window, cx);
                                            }),
                                        )
                                    })),
                            )
                    })),
            )
            .child(self.section_title(
                language.text(
                    "High-Risk Command Guard & Approval Rules",
                    "高危命令拦截与审批规则",
                ),
                colors,
            ))
            .child(
                self.card(colors)
                    .children(settings.mcp.guard_rules.iter().enumerate().map(|(i, rule)| {
                        let rule_id = rule.id;
                        let is_deny = rule.action == McpGuardAction::Deny;
                        div()
                            .px(px(14.))
                            .py(px(10.))
                            .when(i > 0, |d| d.border_t_1().border_color(colors.hairline()))
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(12.))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .min_w_0()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(7.))
                                            .child(
                                                div()
                                                    .text_size(px(13.))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(rgb(colors.foreground))
                                                    .child(rule.label.clone()),
                                            )
                                            .child(
                                                div()
                                                    .px(px(7.))
                                                    .py(px(1.5))
                                                    .rounded_full()
                                                    .bg(if is_deny {
                                                        colors.danger_soft()
                                                    } else {
                                                        colors.warning_soft()
                                                    })
                                                    .text_size(px(10.5))
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_color(rgb(if is_deny {
                                                        colors.danger
                                                    } else {
                                                        colors.warning()
                                                    }))
                                                    .child(if is_deny {
                                                        language.text("Auto-Block", "直接拦截")
                                                    } else {
                                                        language.text("Require Approval", "需人工审批")
                                                    }),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(rgb(colors.secondary))
                                            .child(rule.pattern.clone()),
                                    ),
                            )
                            .child(
                                toggle_switch(("mcp-guard-sw", i), rule.enabled, colors).on_click(
                                    cx.listener(move |this, _, window, cx| {
                                        this.toggle_guard_rule(rule_id, window, cx);
                                    }),
                                ),
                            )
                    }))
                    .child(
                        div()
                            .px(px(14.))
                            .py(px(10.))
                            .border_t_1()
                            .border_color(colors.hairline())
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(input_field(&self.guard_pattern_input, None, colors)),
                            )
                            .child(
                                secondary_button(
                                    "mcp-add-guard",
                                    Some(Icon::Plus),
                                    language.text("Add Approval Rule", "添加审批规则"),
                                    colors,
                                )
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.add_guard_rule(window, cx);
                                })),
                            ),
                    ),
            )
            .child(self.section_title(
                language.text("Session Security & Secret Masking", "会话安全与输出脱敏"),
                colors,
            ))
            .child(
                self.card(colors)
                    .child(
                        div()
                            .px(px(14.))
                            .py(px(11.))
                            .border_b_1()
                            .border_color(colors.hairline())
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(12.))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(rgb(colors.foreground))
                                            .child(language.text(
                                                "Session Max TTL",
                                                "签发会话最大有效期 (TTL)",
                                            )),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(rgb(colors.secondary))
                                            .child(language.text(
                                                "Sessions created via open_host_session are automatically reclaimed on expiry",
                                                "AI 通过 open_host_session 创建的受控会话到期后自动回收",
                                            )),
                                    ),
                            )
                            .child(div().w(px(130.)).child(Select::new(&self.ttl_select).small().text_xs())),
                    )
                    .child(
                        div()
                            .px(px(14.))
                            .py(px(11.))
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(12.))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(rgb(colors.foreground))
                                            .child(language.text(
                                                "Auto-Redact Secrets in Output",
                                                "返回输出自动脱敏密钥",
                                            )),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(rgb(colors.secondary))
                                            .child(language.text(
                                                "Mask private keys, Bearer tokens, AWS keys, and DB connection passwords",
                                                "自动遮蔽返回给 AI 的私钥、Bearer Token、AWS Key 与数据库连接串",
                                            )),
                                    ),
                            )
                            .child(
                                toggle_switch("mcp-mask-sw", settings.mcp.mask_secrets, colors)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.toggle_mask_secrets(window, cx);
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_sessions_tab(
        &self,
        snap: &McpSnapshot,
        colors: Colors,
        language: Language,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .max_w(px(780.))
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(self.section_title(
                        format!(
                            "{} ({})",
                            language.text("Pending Approval Queue", "待人工审批队列"),
                            snap.pending_approvals.len()
                        ),
                        colors,
                    ))
                    .child(
                        secondary_button(
                            "mcp-sim-inline",
                            Some(Icon::Plus),
                            language.text("Simulate AI Request", "模拟 AI 审批请求"),
                            colors,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.simulate_approval(cx);
                        })),
                    ),
            )
            .when(snap.pending_approvals.is_empty(), |d| {
                d.child(
                    self.card(colors).child(
                        div()
                            .px(px(14.))
                            .py(px(12.))
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(colors.foreground))
                                    .child(language.text("No pending approvals", "当前无待审批命令")),
                            )
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(rgb(colors.secondary))
                                    .child(language.text(
                                        "When an AI Agent attempts a protected command, the request appears here immediately.",
                                        "当 AI Agent 尝试在受保护主机执行高危命令时，请求会立即出现在此处并触发通知",
                                    )),
                            ),
                    ),
                )
            })
            .children(snap.pending_approvals.iter().enumerate().map(|(i, req)| {
                let req_id = req.id;
                div()
                    .p(px(14.))
                    .rounded(px(10.))
                    .bg(colors.warning_soft())
                    .border_1()
                    .border_color(colors.warning_border())
                    .flex()
                    .flex_col()
                    .gap(px(9.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(7.))
                                    .text_size(px(12.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(
                                        div()
                                            .px(px(7.))
                                            .py(px(1.5))
                                            .rounded_full()
                                            .bg(colors.warning_soft())
                                            .text_size(px(10.5))
                                            .text_color(rgb(colors.warning()))
                                            .child(language.text("Pending", "待审批")),
                                    )
                                    .child(
                                        div()
                                            .text_color(rgb(colors.foreground))
                                            .child(req.client.clone()),
                                    )
                                    .child(div().text_color(rgb(colors.muted)).child("→"))
                                    .child(
                                        div()
                                            .text_color(rgb(colors.foreground))
                                            .child(req.host.clone()),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(colors.muted))
                                    .child(format!("{} · {}", req.time, req.session_id)),
                            ),
                    )
                    .child(
                        div()
                            .px(px(10.))
                            .py(px(8.))
                            .rounded(px(7.))
                            .bg(colors.term_bg(100))
                            .border_1()
                            .border_color(colors.hairline())
                            .text_size(px(12.))
                            .text_color(rgb(colors.foreground))
                            .child(format!("$ {}", req.cmd)),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(10.))
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(rgb(colors.secondary))
                                    .child(req.reason.clone()),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .child(
                                        danger_button(
                                            ("mcp-deny", i),
                                            None,
                                            language.text("Deny", "拒绝"),
                                            true,
                                            colors,
                                        )
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.resolve_approval(req_id, false, window, cx);
                                        })),
                                    )
                                    .child(
                                        primary_button(
                                            ("mcp-approve", i),
                                            None,
                                            language.text("Approve Once", "批准本次执行"),
                                            colors,
                                        )
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.resolve_approval(req_id, true, window, cx);
                                        })),
                                    ),
                            ),
                    )
            }))
            .child(self.section_title(
                format!(
                    "{} ({})",
                    language.text("Active MCP Sessions", "活跃 MCP 会话"),
                    snap.active_sessions.len()
                ),
                colors,
            ))
            .child(
                self.card(colors)
                    .when(snap.active_sessions.is_empty(), |d| {
                        d.child(
                            div()
                                .px(px(14.))
                                .py(px(12.))
                                .text_size(px(12.))
                                .text_color(rgb(colors.secondary))
                                .child(language.text(
                                    "No active AI sessions right now.",
                                    "当前没有活跃的 AI 会话",
                                )),
                        )
                    })
                    .children(snap.active_sessions.iter().enumerate().map(|(i, s)| {
                        let sid = s.id.clone();
                        let mins = s.ttl_left_secs / 60;
                        div()
                            .px(px(14.))
                            .py(px(10.))
                            .when(i > 0, |d| d.border_t_1().border_color(colors.hairline()))
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(10.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .min_w_0()
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(rgb(colors.secondary))
                                            .child(s.id.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.5))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(rgb(colors.foreground))
                                            .child(s.client.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(rgb(colors.secondary))
                                            .child(s.host.clone()),
                                    )
                                    .child(
                                        div()
                                            .px(px(6.))
                                            .py(px(1.5))
                                            .rounded(px(4.))
                                            .bg(colors.accent_soft())
                                            .text_size(px(10.5))
                                            .text_color(rgb(colors.accent))
                                            .child(s.grant.label(language)),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(rgb(colors.secondary))
                                            .child(format!(
                                                "{mins}m · {} {}",
                                                s.cmds,
                                                language.text("cmds", "次")
                                            )),
                                    )
                                    .child(
                                        danger_button(
                                            ("mcp-kill", i),
                                            None,
                                            language.text("Disconnect", "强制断开"),
                                            true,
                                            colors,
                                        )
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.kill_session(&sid, window, cx);
                                        })),
                                    ),
                            )
                    })),
            )
            .into_any_element()
    }

    fn render_audit_tab(
        &self,
        snap: &McpSnapshot,
        colors: Colors,
        language: Language,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .max_w(px(780.))
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(self.section_title(
                language.text(
                    "MCP Session Audit & Replay (click a row to step through in Inspector)",
                    "MCP 会话审计与操作回放（点击任意行在右侧 Inspector 步进回放）",
                ),
                colors,
            ))
            .child(
                self.card(colors)
                    .when(snap.audit_sessions.is_empty(), |d| {
                        d.child(
                            div()
                                .px(px(14.))
                                .py(px(14.))
                                .text_size(px(12.))
                                .text_color(rgb(colors.secondary))
                                .child(language.text(
                                    "No MCP audit records yet. Tool calls and approvals will be recorded here.",
                                    "暂无 MCP 审计记录，AI 工具调用与审批记录将实时展示在此处。",
                                )),
                        )
                    })
                    .children(snap.audit_sessions.iter().enumerate().map(|(i, sess)| {
                        let selected = self.replay_session_id.as_deref() == Some(sess.id.as_str());
                        let sid = sess.id.clone();
                        let (v_bg, v_fg) = verdict_colors(sess.verdict, colors);

                        div()
                            .id(("mcp-audit-row", i))
                            .px(px(14.))
                            .py(px(10.))
                            .when(i > 0, |d| d.border_t_1().border_color(colors.hairline()))
                            .cursor_pointer()
                            .bg(if selected {
                                colors.accent_soft()
                            } else {
                                transparent_black().into()
                            })
                            .hover(|s| s.bg(colors.surface_hover_glass()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.open_replay(sid.clone(), cx);
                            }))
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(10.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .min_w_0()
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(rgb(colors.secondary))
                                            .child(sess.id.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.5))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(rgb(colors.foreground))
                                            .child(sess.client.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(rgb(colors.secondary))
                                            .child(sess.host.clone()),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(rgb(colors.secondary))
                                            .child(format!(
                                                "{} {}",
                                                sess.events.len(),
                                                language.text("steps", "步")
                                            )),
                                    )
                                    .child(
                                        div()
                                            .px(px(7.))
                                            .py(px(1.5))
                                            .rounded_full()
                                            .bg(v_bg)
                                            .text_size(px(10.5))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(rgb(v_fg))
                                            .child(sess.verdict.label(language)),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(rgb(colors.muted))
                                            .child(sess.started.clone()),
                                    ),
                            )
                    })),
            )
            .into_any_element()
    }

    fn render_replay_inspector(
        &self,
        sess: &McpAuditSession,
        colors: Colors,
        language: Language,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let total = sess.events.len();
        let idx = if total == 0 {
            0
        } else {
            self.replay_step.min(total - 1)
        };
        let cur = sess.events.get(idx);
        let pct = if total == 0 {
            0.0
        } else {
            ((idx + 1) as f32 / total as f32) * 100.0
        };

        div()
            .id("mcp-replay-drawer")
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .absolute()
            .top_0()
            .bottom_0()
            .right_0()
            .w(px(330.))
            .bg(colors.popover_bg())
            .border_l_1()
            .border_color(colors.hairline_strong())
            .shadow_lg()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(
                div()
                    .h(px(42.))
                    .px(px(14.))
                    .border_b_1()
                    .border_color(colors.hairline())
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(colors.foreground))
                            .child(format!(
                                "{} · {}",
                                language.text("Replay", "回放"),
                                sess.id
                            )),
                    )
                    .child(
                        Button::new("mcp-replay-close")
                            .ghost()
                            .xsmall()
                            .icon(Icon::Close)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.stop_replay();
                                this.replay_session_id = None;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .id("mcp-replay-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(px(14.))
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(self.section_title(
                                language.text("Session Summary", "会话摘要"),
                                colors,
                            ))
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .text_size(px(12.))
                                    .child(
                                        div()
                                            .text_color(rgb(colors.secondary))
                                            .child(language.text("Client", "客户端")),
                                    )
                                    .child(
                                        div()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(rgb(colors.foreground))
                                            .child(sess.client.clone()),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .text_size(px(12.))
                                    .child(
                                        div()
                                            .text_color(rgb(colors.secondary))
                                            .child(language.text("Target Host", "目标主机")),
                                    )
                                    .child(
                                        div()
                                            .text_color(rgb(colors.foreground))
                                            .child(sess.host.clone()),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .text_size(px(12.))
                                    .child(
                                        div()
                                            .text_color(rgb(colors.secondary))
                                            .child(language.text("Progress", "进度")),
                                    )
                                    .child(
                                        div()
                                            .text_color(rgb(colors.foreground))
                                            .child(format!("{} / {total}", idx + 1)),
                                    ),
                            )
                            .child(
                                div()
                                    .h(px(4.))
                                    .rounded_full()
                                    .bg(colors.surface_active_glass())
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .h_full()
                                            .w(gpui_kit::relative(pct / 100.0))
                                            .bg(rgb(colors.accent)),
                                    ),
                            ),
                    )
                    .children(cur.map(|ev| {
                        let (v_bg, v_fg) = verdict_colors(ev.verdict, colors);
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(self.section_title(
                                language.text(
                                    "Step Output (Secrets Redacted)",
                                    "当前步骤输出（已脱敏）",
                                ),
                                colors,
                            ))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(rgb(colors.accent))
                                            .child(ev.tool.clone()),
                                    )
                                    .child(
                                        div()
                                            .px(px(7.))
                                            .py(px(1.5))
                                            .rounded_full()
                                            .bg(v_bg)
                                            .text_size(px(10.5))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(rgb(v_fg))
                                            .child(ev.verdict.step_label(language)),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(rgb(colors.foreground))
                                    .child(ev.summary.clone()),
                            )
                            .child(
                                div()
                                    .p(px(9.))
                                    .rounded(px(7.))
                                    .bg(colors.term_bg(100))
                                    .border_1()
                                    .border_color(colors.hairline())
                                    .text_size(px(11.))
                                    .text_color(rgb(colors.foreground))
                                    .child(ev.output.clone()),
                            )
                    }))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(self.section_title(
                                language.text("Full Timeline", "完整操作时间线"),
                                colors,
                            ))
                            .children(sess.events.iter().enumerate().map(|(i, ev)| {
                                let active = i == idx;
                                div()
                                    .id(("mcp-step", i))
                                    .p(px(9.))
                                    .rounded(px(8.))
                                    .cursor_pointer()
                                    .bg(if active {
                                        colors.accent_soft()
                                    } else {
                                        colors.surface_glass()
                                    })
                                    .border_1()
                                    .border_color(if active {
                                        colors.accent_border()
                                    } else {
                                        colors.hairline()
                                    })
                                    .hover(|s| s.bg(colors.surface_hover_glass()))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.stop_replay();
                                        this.replay_step = i;
                                        cx.notify();
                                    }))
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .child(
                                                div()
                                                    .text_size(px(11.5))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(rgb(colors.foreground))
                                                    .child(format!("{}. {}", i + 1, ev.tool)),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(10.))
                                                    .text_color(rgb(colors.muted))
                                                    .child(ev.ts.clone()),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(px(11.))
                                            .text_color(rgb(colors.secondary))
                                            .child(ev.summary.clone()),
                                    )
                            })),
                    ),
            )
            .child(
                div()
                    .px(px(14.))
                    .py(px(10.))
                    .border_t_1()
                    .border_color(colors.hairline())
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(6.))
                    .child(
                        secondary_button(
                            "mcp-replay-prev",
                            None,
                            language.text("← Prev", "← 上一步"),
                            colors,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.step_replay(-1, total, cx);
                        })),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                primary_button(
                                    "mcp-replay-play",
                                    None,
                                    if self.replay_playing {
                                        language.text("Pause", "暂停")
                                    } else {
                                        language.text("Auto Play", "自动回放")
                                    },
                                    colors,
                                )
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.toggle_replay_play(total, window, cx);
                                })),
                            )
                            .child(
                                secondary_button(
                                    "mcp-replay-next",
                                    None,
                                    language.text("Next →", "下一步 →"),
                                    colors,
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.step_replay(1, total, cx);
                                })),
                            ),
                    ),
            )
            .into_any_element()
    }
}

fn verdict_colors(verdict: AuditVerdict, colors: Colors) -> (gpui_kit::Rgba, u32) {
    match verdict {
        AuditVerdict::Allow => (colors.success_soft(), colors.success),
        AuditVerdict::Approved => (colors.accent_soft(), colors.accent),
        AuditVerdict::Denied => (colors.danger_soft(), colors.danger),
        AuditVerdict::Pending => (colors.warning_soft(), colors.warning()),
    }
}
