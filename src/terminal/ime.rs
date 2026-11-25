use std::ops::Range;

use gpui_kit::{
    App, Bounds, Context, ElementInputHandler, EntityInputHandler, IntoElement, ParentElement as _,
    Pixels, Point, Size, Styled as _, UTF16Selection, Window, canvas, div, font, point, px, rgb,
    size,
};

use super::{TerminalSession, TerminalStatus};

#[derive(Default)]
pub(super) struct Composition {
    text: Vec<u16>,
    selection: Range<usize>,
}

impl Composition {
    fn replace(&mut self, range: Option<Range<usize>>, text: &str) -> Range<usize> {
        let range = range.unwrap_or(0..self.text.len());
        let start = range.start.min(self.text.len());
        let end = range.end.clamp(start, self.text.len());
        self.text.splice(start..end, text.encode_utf16());
        start..start + text.encode_utf16().count()
    }
}

impl TerminalSession {
    pub(super) fn ime_overlay(&self, cx: &Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let focus = self.focus.clone();
        let input = canvas(
            |_, _, _| (),
            move |bounds, (), window, cx| {
                window.handle_input(&focus, ElementInputHandler::new(bounds, entity), cx);
            },
        )
        .absolute()
        .size_full();
        let mut overlay = div().absolute().size_full().child(input);

        if !self.composition.text.is_empty()
            && let Some(view) = &self.terminal
        {
            let style = view.read(cx).style();
            let (origin, cell_size) = self.ime_cursor(cx);
            overlay = overlay.child(
                div()
                    .absolute()
                    .left(origin.x)
                    .top(origin.y)
                    .h(cell_size.height)
                    .line_height(cell_size.height)
                    .font_family(style.font_family.clone())
                    .text_size(style.font_size)
                    .underline()
                    .bg(rgb(self.palette.background))
                    .child(String::from_utf16_lossy(&self.composition.text)),
            );
        }
        overlay
    }

    fn ime_cursor(&self, cx: &App) -> (Point<Pixels>, Size<Pixels>) {
        let Some(view) = &self.terminal else {
            return (point(px(0.), px(0.)), size(px(1.), px(1.)));
        };
        let view = view.read(cx);
        let style = view.style();
        let text = cx.text_system();
        let face = text.resolve_font(&font(style.font_family.clone()));
        let width = text.em_advance(face, style.font_size).unwrap_or(px(8.));
        let height = px((f32::from(text.ascent(face, style.font_size))
            + f32::from(text.descent(face, style.font_size)).abs())
        .ceil());
        let (column, row) = view.terminal().with_grid(|grid| grid.cursor());
        (
            point(width * column as f32, height * row as f32),
            size(width, height),
        )
    }
}

impl EntityInputHandler for TerminalSession {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let text = self.composition.text.get(range.clone())?;
        *actual = Some(range);
        String::from_utf16(text).ok()
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.composition.selection.clone(),
            reversed: false,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        (!self.composition.text.is_empty()).then_some(0..self.composition.text.len())
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.composition = Composition::default();
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.composition.replace(range, text);
        let committed = String::from_utf16_lossy(&self.composition.text);
        self.unmark_text(window, cx);
        if !committed.is_empty()
            && let Some(view) = &self.terminal
            && let Err(error) = view.read(cx).terminal().write(committed.as_bytes())
        {
            self.status = Some(TerminalStatus::IoError(error.to_string()));
        }
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selection: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let inserted = self.composition.replace(range, text);
        self.composition.selection = match selection {
            Some(selection) => {
                (inserted.start + selection.start.min(inserted.len()))
                    ..(inserted.start + selection.end.min(inserted.len()))
            }
            None => inserted.end..inserted.end,
        };
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let (origin, cell_size) = self.ime_cursor(cx);
        Some(Bounds {
            origin: bounds.origin + origin,
            size: cell_size,
        })
    }

    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }

    fn accepts_text_input(&self, _: &mut Window, cx: &mut Context<Self>) -> bool {
        self.terminal
            .as_ref()
            .is_some_and(|view| view.read(cx).terminal().exit_report().is_none())
    }
}

#[cfg(test)]
mod tests {
    use super::Composition;

    #[test]
    fn composition_replaces_platform_utf16_ranges() {
        let mut composing = Composition::default();
        assert_eq!(composing.replace(None, "ni😀hao"), 0..7);
        assert_eq!(composing.replace(Some(2..4), "你"), 2..3);
        assert_eq!(String::from_utf16(&composing.text).unwrap(), "ni你hao");
    }
}
