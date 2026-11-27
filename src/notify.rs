use std::{
    cell::RefCell,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

use gpui_kit::{
    App, AppContext as _, Focusable as _, IntoElement, ParentElement as _, SharedString,
    Styled as _, Subscription, Window,
    component::{
        Sizable as _, WindowExt as _,
        button::{Button, ButtonVariants as _},
        input::{Input, InputEvent, InputState},
        notification::{Notification, NotificationType},
    },
    div, px,
};
use tokio::sync::oneshot;

use crate::icons::Icon;
use crate::settings::Language;
use crate::ssh::{SshPrompt, SshPromptKind};

struct SshPromptNotification;
struct TransientToast;
struct UpdateNotification;

static NEXT_PROMPT_ID: AtomicU64 = AtomicU64::new(1);

pub fn info(message: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
    window.push_notification(Notification::info(message).id::<TransientToast>(), cx);
}

pub fn success(message: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
    window.push_notification(Notification::success(message).id::<TransientToast>(), cx);
}

#[allow(dead_code)]
pub fn warning(message: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
    window.push_notification(Notification::warning(message).id::<TransientToast>(), cx);
}

pub fn error(message: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
    window.push_notification(Notification::error(message), cx);
}

pub fn error_action(
    title: impl Into<SharedString>,
    message: impl Into<SharedString>,
    action_label: impl Into<SharedString>,
    action_icon: Option<Icon>,
    on_action: impl Fn(&mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    let message = message.into();
    let action_label = action_label.into();
    let on_action = Rc::new(on_action);
    let note = Notification::new()
        .with_type(NotificationType::Error)
        .title(title)
        .autohide(false)
        .content(move |_, _, cx| {
            let on_action = on_action.clone();
            let mut btn = Button::new("error-notification-action")
                .primary()
                .xsmall()
                .h(px(24.))
                .px(px(8.))
                .label(action_label.clone())
                .on_click(cx.listener(move |note, _, window, cx| {
                    on_action(window, cx);
                    note.dismiss(window, cx);
                }));
            if let Some(icon) = action_icon {
                btn = btn.icon(icon);
            }
            div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .pt(px(4.))
                .w_full()
                .child(div().w_full().text_size(px(12.)).child(message.clone()))
                .child(div().flex().items_center().justify_end().child(btn))
                .into_any_element()
        });
    window.push_notification(note, cx);
}

pub fn update_action(
    title: impl Into<SharedString>,
    message: impl Into<SharedString>,
    action_label: impl Into<SharedString>,
    on_action: impl Fn(&mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    let message = message.into();
    let action_label = action_label.into();
    let on_action = Rc::new(on_action);
    let note = Notification::new()
        .with_type(NotificationType::Info)
        .id::<UpdateNotification>()
        .title(title)
        .autohide(false)
        .content(move |_, _, cx| {
            let on_action = on_action.clone();
            div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .pt(px(4.))
                .w_full()
                .child(div().w_full().text_size(px(12.)).child(message.clone()))
                .child(
                    div().flex().items_center().justify_end().child(
                        Button::new("update-notification-action")
                            .primary()
                            .xsmall()
                            .h(px(24.))
                            .px(px(8.))
                            .label(action_label.clone())
                            .on_click(cx.listener(move |note, _, window, cx| {
                                on_action(window, cx);
                                note.dismiss(window, cx);
                            })),
                    ),
                )
                .into_any_element()
        });
    window.push_notification(note, cx);
}

pub fn confirm(
    title: impl Into<SharedString>,
    message: impl Into<SharedString>,
    confirm_label: impl Into<SharedString>,
    on_confirm: impl Fn(&mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    let message = message.into();
    let confirm_label = confirm_label.into();
    let on_confirm = Rc::new(on_confirm);
    let note = Notification::new()
        .with_type(NotificationType::Warning)
        .title(title)
        .autohide(false)
        .content(move |_, _, cx| {
            let on_confirm = on_confirm.clone();
            div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .pt(px(4.))
                .w_full()
                .child(div().w_full().text_size(px(12.)).child(message.clone()))
                .child(
                    div().flex().items_center().justify_end().child(
                        Button::new("confirm-notification-action")
                            .danger()
                            .xsmall()
                            .h(px(24.))
                            .px(px(8.))
                            .label(confirm_label.clone())
                            .on_click(cx.listener(move |note, _, window, cx| {
                                on_confirm(window, cx);
                                note.dismiss(window, cx);
                            })),
                    ),
                )
                .into_any_element()
        });
    window.push_notification(note, cx);
}

pub fn push_ssh_prompt(
    prompt: SshPrompt,
    language: Language,
    window: &mut Window,
    cx: &mut App,
) -> Option<Subscription> {
    let prompt_id = NEXT_PROMPT_ID.fetch_add(1, Ordering::Relaxed);
    let reply: Rc<RefCell<Option<oneshot::Sender<Option<String>>>>> =
        Rc::new(RefCell::new(Some(prompt.reply)));

    let (title, detail, placeholder) = match &prompt.kind {
        SshPromptKind::Password { endpoint } => (
            language.text("Password Required", "需要密码"),
            format!(
                "{} {endpoint}",
                language.text("Password required for", "请输入密码：")
            ),
            Some(language.text("Enter password…", "输入密码…")),
        ),
        SshPromptKind::Passphrase { key_path } => (
            language.text("Passphrase Required", "需要私钥口令"),
            format!(
                "{} {key_path}",
                language.text("Passphrase required for", "请输入私钥口令：")
            ),
            Some(language.text("Enter passphrase…", "输入私钥口令…")),
        ),
        SshPromptKind::RelayPassword { command, endpoint } => (
            language.text("Relay Password Required", "需要中继命令密码"),
            format!(
                "{} `{command}` ({endpoint})",
                language.text("Password required for", "请输入中继命令密码：")
            ),
            Some(language.text("Enter relay password…", "输入中继密码…")),
        ),
        SshPromptKind::HostKey {
            host,
            port,
            fingerprint,
        } => (
            language.text("Host Key Verification", "主机指纹验证"),
            format!(
                "{} {host}:{port} ({fingerprint})",
                language.text("Trust host key for", "是否信任主机指纹")
            ),
            None,
        ),
    };

    let input = placeholder.map(|ph| cx.new(|cx| InputState::new(window, cx).placeholder(ph).masked(true)));
    if let Some(input) = &input {
        input.focus_handle(cx).focus(window, cx);
    }

    let confirm_label = match &prompt.kind {
        SshPromptKind::HostKey { .. } => language.text("Trust & Save", "信任并保存"),
        _ => language.text("Confirm", "确认"),
    };
    let cancel_label = language.text("Cancel", "取消");

    let enter_sub = input.as_ref().map(|input| {
        let reply = reply.clone();
        window.subscribe(input, cx, move |input, event, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                if let Some(tx) = reply.borrow_mut().take() {
                    let _ = tx.send(Some(input.read(cx).value().to_string()));
                }
                window.remove_notification::<SshPromptNotification>(cx);
            }
        })
    });

    let reply_submit = reply.clone();
    let reply_cancel = reply.clone();
    let reply_close = reply;
    let input_for_content = input;

    let note = Notification::new()
        .id1::<SshPromptNotification>(("ssh-prompt", prompt_id as usize))
        .with_type(NotificationType::Warning)
        .title(title)
        .autohide(false)
        .on_close(move |_, _| {
            if let Some(tx) = reply_close.borrow_mut().take() {
                let _ = tx.send(None);
            }
        })
        .content(move |_, _, cx| {
            let reply_submit = reply_submit.clone();
            let reply_cancel = reply_cancel.clone();
            let input_submit = input_for_content.clone();

            let mut col = div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .w_full()
                .child(div().text_size(px(12.)).child(detail.clone()));

            if let Some(input) = &input_for_content {
                col = col.child(Input::new(input).small().text_xs().mask_toggle());
            }

            col = col.child(
                div()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(6.))
                    .child(
                        Button::new(("ssh-prompt-cancel", prompt_id as usize))
                            .secondary()
                            .xsmall()
                            .h(px(24.))
                            .px(px(8.))
                            .label(cancel_label)
                            .on_click(cx.listener(move |note, _, window, cx| {
                                if let Some(tx) = reply_cancel.borrow_mut().take() {
                                    let _ = tx.send(None);
                                }
                                note.dismiss(window, cx);
                            })),
                    )
                    .child(
                        Button::new(("ssh-prompt-ok", prompt_id as usize))
                            .primary()
                            .xsmall()
                            .h(px(24.))
                            .px(px(8.))
                            .label(confirm_label)
                            .on_click(cx.listener(move |note, _, window, cx| {
                                if let Some(tx) = reply_submit.borrow_mut().take() {
                                    let answer = input_submit
                                        .as_ref()
                                        .map(|i| i.read(cx).value().to_string())
                                        .unwrap_or_else(|| "trust".to_owned());
                                    let _ = tx.send(Some(answer));
                                }
                                note.dismiss(window, cx);
                            })),
                    ),
            );

            col.into_any_element()
        });

    window.push_notification(note, cx);
    enter_sub
}
