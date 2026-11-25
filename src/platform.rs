use gpui_kit::{App, MouseDownEvent, Window};

pub fn drag_window(event: &MouseDownEvent, window: &mut Window, _: &mut App) {
    if event.click_count == 2 {
        window.zoom_window();
    } else {
        window.start_window_move();
    }
}

#[cfg(target_os = "macos")]
pub fn apply_window_style(window: &Window, opacity: u8) {
    use objc2_app_kit::{NSView, NSWindowButton};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };
    // GPUI provides a live NSView borrowed from this window on the AppKit thread.
    let view = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
    let Some(native) = view.window() else {
        return;
    };
    for kind in [
        NSWindowButton::CloseButton,
        NSWindowButton::MiniaturizeButton,
        NSWindowButton::ZoomButton,
    ] {
        if let Some(button) = native.standardWindowButton(kind)
            && !button.isHidden()
        {
            button.setHidden(true);
        }
    }
    native.setAlphaValue(f64::from(opacity) / 100.);
}

#[cfg(not(target_os = "macos"))]
pub fn apply_window_style(window: &Window, opacity: u8) {
    use gpui_kit::WindowBackgroundAppearance;

    window.set_background_appearance(if opacity == 100 {
        WindowBackgroundAppearance::Opaque
    } else {
        WindowBackgroundAppearance::Transparent
    });
}
