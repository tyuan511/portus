//! The view entity: its state, its pump, and what a host can ask of it.

use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{
    App, Bounds, Context, EventEmitter, FocusHandle, Focusable, Pixels, Point, Size, Subscription,
    Task, Window, px, size,
};

use super::TerminalStyle;
use super::metrics::CellMetrics;
use super::render::RowRenderCache;
use crate::{Result, Terminal, TerminalEvent, Transport};

const CURSOR_BLINK_INTERVAL: Duration = Duration::from_millis(530);
const SYNC_UPDATE_SAFETY_TIMER: Duration = Duration::from_millis(64);

/// Draws a [`Terminal`] and routes input to it.
pub struct TerminalView<T> {
    pub(super) terminal:          Terminal<T>,
    pub(super) focus:             FocusHandle,
    pub(super) style:             TerminalStyle,
    pub(super) metrics:           Option<CellMetrics>,
    pub(super) origin:            Point<Pixels>,
    pub(super) scroll_remainder:  f32,
    pub(super) selection_dragged: bool,
    pub(super) focused:           bool,
    pub(super) cursor_blink_on:   bool,
    pub(super) row_cache:         RowRenderCache,
    blink_epoch:                  u64,
    _blink_task:                  Option<Task<()>>,
    _sync_fallback:               Option<Task<()>>,
    focus_subs:                   Option<(Subscription, Subscription)>,
    exit_reported:                bool,
    _pump:                        Task<()>,
    _keys:                        Subscription,
}

impl<T: Transport> TerminalView<T> {
    pub fn new(terminal: Terminal<T>, style: TerminalStyle, cx: &mut Context<Self>) -> Self {
        terminal.set_palette(style.palette);
        let wake = terminal.wake();
        let pump = cx.spawn(async move |view, cx| {
                         while wake.recv().await.is_ok() {
                             if view.update(cx, |view, cx| view.pump(cx)).is_err() {
                                 break;
                             }
                         }
                     });
        let focus = cx.focus_handle();
        let keys = Self::take_keys(focus.clone(), cx);
        Self { terminal,
               focus,
               style,
               metrics: None,
               origin: Point::default(),
               scroll_remainder: 0.,
               selection_dragged: false,
               focused: false,
               cursor_blink_on: true,
               row_cache: RowRenderCache::default(),
               blink_epoch: 0,
               _blink_task: None,
               _sync_fallback: None,
               focus_subs: None,
               exit_reported: false,
               _pump: pump,
               _keys: keys }
    }

    fn take_keys(focus: FocusHandle, cx: &mut Context<Self>) -> Subscription {
        let view = cx.weak_entity();
        cx.intercept_keystrokes(move |event, window, cx| {
              if !focus.is_focused(window) {
                  return;
              }
              let taken = view.update(cx, |view, cx| view.handle_key(&event.keystroke, cx))
                              .unwrap_or(false);
              if taken {
                  cx.stop_propagation();
              }
          })
    }

    pub fn terminal(&self) -> &Terminal<T> {
        &self.terminal
    }

    pub fn style(&self) -> &TerminalStyle {
        &self.style
    }

    /// Measured size of one terminal cell in pixels.
    pub fn cell_size(&self, cx: &App) -> Size<Pixels> {
        let metrics = self.metrics
                          .unwrap_or_else(|| CellMetrics::measure(&self.style, cx));
        size(px(metrics.width), px(metrics.height))
    }

    /// Changes the style, re-measuring and refitting on the next frame when
    /// typography metrics changed. A no-op when `style` is already in use.
    pub fn set_style(&mut self, style: TerminalStyle, cx: &mut Context<Self>) {
        if self.style != style {
            let metrics_changed = self.style.font_family != style.font_family
                                  || self.style.font_size != style.font_size
                                  || self.style.line_height != style.line_height;
            let blink_changed = self.style.cursor_blink != style.cursor_blink
                                || self.style.cursor_style != style.cursor_style;
            self.terminal.set_palette(style.palette);
            self.style = style;
            self.row_cache.clear();
            if metrics_changed {
                self.metrics = None;
            }
            if blink_changed {
                if self.focused && self.style.cursor_blink {
                    self.reset_cursor_blink(cx);
                }
                else {
                    self.stop_cursor_blink();
                }
            }
            cx.notify();
        }
    }

    /// Ensures focus/blur listeners are registered and focus state is synced.
    pub(super) fn sync_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.focus_subs.is_none() {
            let focus_handle = self.focus.clone();
            let on_focus = cx.on_focus(&focus_handle, window, |this, _, cx| {
                                 this.focused = true;
                                 this.reset_cursor_blink(cx);
                                 this.send_focus_event(true, cx);
                                 cx.notify();
                             });
            let on_blur = cx.on_blur(&focus_handle, window, |this, _, cx| {
                                this.focused = false;
                                this.stop_cursor_blink();
                                this.send_focus_event(false, cx);
                                cx.notify();
                            });
            self.focus_subs = Some((on_focus, on_blur));
        }
        let is_focused = self.focus.is_focused(window);
        if self.focused != is_focused {
            self.focused = is_focused;
            if is_focused && self.style.cursor_blink {
                self.reset_cursor_blink(cx);
            }
            else {
                self.stop_cursor_blink();
            }
            self.send_focus_event(is_focused, cx);
        }
    }

    fn send_focus_event(&self, focused: bool, cx: &mut Context<Self>) {
        if self.terminal.exit_report().is_some() {
            return;
        }
        if self.terminal.with_grid(|grid| grid.focus_reporting()) {
            let seq = if focused { b"\x1b[I" } else { b"\x1b[O" };
            let written = self.terminal.write(seq);
            self.report(written, cx);
        }
    }

    /// Resets the cursor to visible and (re)starts the blink timer if focused
    /// and blinking is enabled.
    pub(super) fn reset_cursor_blink(&mut self, cx: &mut Context<Self>) {
        self.cursor_blink_on = true;
        self.blink_epoch = self.blink_epoch.wrapping_add(1);
        if !self.focused || !self.style.cursor_blink {
            self._blink_task = None;
            return;
        }
        let epoch = self.blink_epoch;
        self._blink_task = Some(cx.spawn(async move |view, cx| {
                                    loop {
                                        cx.background_executor()
                                          .timer(CURSOR_BLINK_INTERVAL)
                                          .await;
                                        let keep_going =
                                            view.update(cx, |this, cx| {
                                                    if this.blink_epoch != epoch
                                                       || !this.focused
                                                       || !this.style.cursor_blink
                                                    {
                                                        return false;
                                                    }
                                                    this.cursor_blink_on = !this.cursor_blink_on;
                                                    cx.notify();
                                                    true
                                                })
                                                .unwrap_or(false);
                                        if !keep_going {
                                            break;
                                        }
                                    }
                                }));
    }

    pub(super) fn stop_cursor_blink(&mut self) {
        self.cursor_blink_on = true;
        self.blink_epoch = self.blink_epoch.wrapping_add(1);
        self._blink_task = None;
    }

    /// Hands what the transport delivered to the frame: a repaint if the grid
    /// changed (deferred while DECSET 2026 synchronized output is active), the
    /// program's requests as events, and the exit, once.
    fn pump(&mut self, cx: &mut Context<Self>) {
        for event in self.terminal.drain_events() {
            cx.emit(event);
        }
        if !self.exit_reported
           && let Some(report) = self.terminal.exit_report()
        {
            self.exit_reported = true;
            cx.emit(TerminalEvent::Exited(report));
        }

        if self.terminal.sync_update_active() {
            if self._sync_fallback.is_none() {
                self._sync_fallback = Some(cx.spawn(async move |view, cx| {
                                               cx.background_executor()
                                                 .timer(SYNC_UPDATE_SAFETY_TIMER)
                                                 .await;
                                               let _ = view.update(cx, |this, cx| {
                                                               this._sync_fallback = None;
                                                               this.pump(cx);
                                                           });
                                           }));
            }
            return;
        }

        self._sync_fallback = None;
        if self.terminal.take_dirty() {
            if self.focused && self.style.cursor_blink {
                if self._blink_task.is_some() {
                    self.cursor_blink_on = true;
                }
                else {
                    self.reset_cursor_blink(cx);
                }
            }
            cx.notify();
        }
    }

    pub(super) fn cell_metrics(&mut self, cx: &App) -> CellMetrics {
        let metrics = *self.metrics
                           .get_or_insert_with(|| CellMetrics::measure(&self.style, cx));
        self.terminal
            .set_cell_size_px(metrics.width.round().max(1.0) as u16,
                              metrics.height.round().max(1.0) as u16);
        metrics
    }

    /// Sizes the terminal to `bounds`, the space its grid was laid out in.
    pub(super) fn fit(&mut self, bounds: Bounds<Pixels>, window: &mut Window,
                      cx: &mut Context<Self>) {
        self.origin = bounds.origin;
        let Some(metrics) = self.metrics
        else {
            return;
        };
        let size = metrics.grid_size(bounds.size);
        if size != self.terminal.size() {
            let resized = self.terminal.resize(size);
            self.report(resized, cx);
            window.defer(cx, |window, _| window.refresh());
        }
    }

    /// Hands a failed transport call to the host.
    pub(super) fn report(&self, result: Result<()>, cx: &mut Context<Self>) {
        if let Err(error) = result {
            cx.emit(TerminalEvent::TransportFailed(Arc::new(error)));
        }
    }
}

impl<T: Transport> EventEmitter<TerminalEvent> for TerminalView<T> {}

impl<T: Transport> Focusable for TerminalView<T> {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}
