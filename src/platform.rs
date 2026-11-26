use gpui_kit::{App, MouseDownEvent, Window, WindowBackgroundAppearance};

pub fn drag_window(event: &MouseDownEvent, window: &mut Window, _: &mut App) {
    if event.click_count == 2 {
        window.zoom_window();
    } else {
        window.start_window_move();
    }
}

pub fn window_background_appearance(opacity: u8, vibrancy: bool) -> WindowBackgroundAppearance {
    if vibrancy {
        WindowBackgroundAppearance::Blurred
    } else if opacity < 100 {
        WindowBackgroundAppearance::Transparent
    } else {
        WindowBackgroundAppearance::Opaque
    }
}

#[cfg(target_os = "macos")]
pub fn hide_traffic_lights(window: &Window) {
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
}

#[cfg(not(target_os = "macos"))]
pub fn hide_traffic_lights(_window: &Window) {}

pub fn apply_window_style(window: &Window, opacity: u8, vibrancy: bool) {
    window.set_background_appearance(window_background_appearance(opacity, vibrancy));
    hide_traffic_lights(window);
}

#[cfg(target_os = "macos")]
pub fn set_app_icon() {
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let data = NSData::with_bytes(crate::icons::APP_LOGO_BYTES);
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    unsafe { app.setApplicationIconImage(Some(&image)) };
}

#[cfg(not(target_os = "macos"))]
pub fn set_app_icon() {}
