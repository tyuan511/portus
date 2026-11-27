use gpui_kit::{
    App, MouseDownEvent, Styled as _, Window, WindowBackgroundAppearance, base::Root,
    transparent_black,
};

pub fn drag_window(event: &MouseDownEvent, window: &mut Window, _: &mut App) {
    if event.click_count == 2 {
        window.zoom_window();
    } else {
        window.start_window_move();
    }
}

pub fn window_background_appearance(opacity: u8) -> WindowBackgroundAppearance {
    if opacity < 100 {
        WindowBackgroundAppearance::Blurred
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

#[cfg(target_os = "macos")]
fn sync_macos_window_vibrancy(window: &Window, vibrancy: bool, dark: bool) {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2_app_kit::NSView;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    const PORTUS_VIBRANCY_ID: &std::ffi::CStr = c"portus.native.vibrancy";
    const NS_VIEW_WIDTH_SIZABLE: usize = 1 << 1;
    const NS_VIEW_HEIGHT_SIZABLE: usize = 1 << 4;
    const NS_WINDOW_BELOW: isize = -1;
    // `gpui-pre-macos 0.3.7` creates a custom `BlurredView` (`NSVisualEffectView`
    // subclass) that both defaults to `Selection` (no `CABackdropLayer` on macOS
    // 27+) and strips all tint/fill layers in `updateLayer`. That leaves a raw,
    // untinted blur where bright windows behind Portus wash out dark-mode text.
    // Mounting a stock `NSVisualEffectView` with `HUDWindow` (13) + `BehindWindow`
    // (0) + `Active` (1) preserves macOS's native DarkAqua/Aqua frosted tint and
    // color saturation while hiding `BlurredView` to avoid double blur passes.
    const NS_VISUAL_EFFECT_MATERIAL_HUD_WINDOW: isize = 13;
    const NS_VISUAL_EFFECT_BLENDING_BEHIND_WINDOW: isize = 0;
    const NS_VISUAL_EFFECT_STATE_ACTIVE: isize = 1;

    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };
    let view = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
    let Some(native) = view.window() else {
        return;
    };

    // Match the native window's NSAppearance to Portus's active theme so
    // macOS vibrancy uses DarkAqua (dark frosted glass) in dark themes even
    // when the system is in Light mode, avoiding washed-out bright backdrops.
    let appearance: *mut AnyObject = unsafe {
        if let (Some(ns_string_cls), Some(ns_appearance_cls)) = (
            AnyClass::get(c"NSString"),
            AnyClass::get(c"NSAppearance"),
        ) {
            let name_cstr = if dark {
                c"NSAppearanceNameDarkAqua"
            } else {
                c"NSAppearanceNameAqua"
            };
            let name: *mut AnyObject =
                msg_send![ns_string_cls, stringWithUTF8String: name_cstr.as_ptr()];
            let appearance: *mut AnyObject = if !name.is_null() {
                msg_send![ns_appearance_cls, appearanceNamed: name]
            } else {
                std::ptr::null_mut()
            };
            if !appearance.is_null() {
                let win_obj: &AnyObject = &native;
                let _: () = msg_send![win_obj, setAppearance: appearance];
            }
            appearance
        } else {
            std::ptr::null_mut()
        }
    };

    let Some(content_view) = native.contentView() else {
        return;
    };
    let (Some(ns_string_cls), Some(vev_cls)) = (
        AnyClass::get(c"NSString"),
        AnyClass::get(c"NSVisualEffectView"),
    ) else {
        return;
    };

    unsafe {
        let portus_id: *mut AnyObject =
            msg_send![ns_string_cls, stringWithUTF8String: PORTUS_VIBRANCY_ID.as_ptr()];
        let mut portus_vev: *mut AnyObject = std::ptr::null_mut();
        let mut gpui_blurred_views: Vec<*mut AnyObject> = Vec::new();

        for subview in content_view.subviews() {
            let obj: &AnyObject = &subview;
            let is_visual_effect: bool = msg_send![obj, isKindOfClass: vev_cls];
            if !is_visual_effect {
                continue;
            }
            let ident: *mut AnyObject = msg_send![obj, identifier];
            let is_portus = !ident.is_null() && !portus_id.is_null() && {
                let eq: bool = msg_send![ident, isEqualToString: portus_id];
                eq
            };
            let ptr = (obj as *const AnyObject).cast_mut();
            if is_portus {
                portus_vev = ptr;
            } else {
                gpui_blurred_views.push(ptr);
            }
        }

        if !vibrancy {
            if !portus_vev.is_null() {
                let _: () = msg_send![portus_vev, removeFromSuperview];
            }
            return;
        }

        for bv in gpui_blurred_views {
            let _: () = msg_send![bv, setHidden: true];
        }

        if portus_vev.is_null() {
            let alloc: *mut AnyObject = msg_send![vev_cls, alloc];
            if alloc.is_null() {
                return;
            }
            let frame = content_view.bounds();
            let vev: *mut AnyObject = msg_send![alloc, initWithFrame: frame];
            if vev.is_null() {
                return;
            }
            if !portus_id.is_null() {
                let _: () = msg_send![vev, setIdentifier: portus_id];
            }
            let mask = NS_VIEW_WIDTH_SIZABLE | NS_VIEW_HEIGHT_SIZABLE;
            let _: () = msg_send![vev, setAutoresizingMask: mask];
            if !appearance.is_null() {
                let _: () = msg_send![vev, setAppearance: appearance];
            }
            let _: () = msg_send![vev, setMaterial: NS_VISUAL_EFFECT_MATERIAL_HUD_WINDOW];
            let _: () =
                msg_send![vev, setBlendingMode: NS_VISUAL_EFFECT_BLENDING_BEHIND_WINDOW];
            let _: () = msg_send![vev, setState: NS_VISUAL_EFFECT_STATE_ACTIVE];
            let cv_obj: &AnyObject = &content_view;
            let nil: *mut AnyObject = std::ptr::null_mut();
            let _: () = msg_send![
                cv_obj,
                addSubview: vev,
                positioned: NS_WINDOW_BELOW,
                relativeTo: nil
            ];
            let _: () = msg_send![vev, release];
        } else {
            if !appearance.is_null() {
                let _: () = msg_send![portus_vev, setAppearance: appearance];
            }
            let _: () = msg_send![portus_vev, setMaterial: NS_VISUAL_EFFECT_MATERIAL_HUD_WINDOW];
            let _: () =
                msg_send![portus_vev, setBlendingMode: NS_VISUAL_EFFECT_BLENDING_BEHIND_WINDOW];
            let _: () = msg_send![portus_vev, setState: NS_VISUAL_EFFECT_STATE_ACTIVE];
            let _: () = msg_send![portus_vev, setHidden: false];
            let _: () = msg_send![portus_vev, setNeedsDisplay: true];
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn sync_macos_window_vibrancy(_window: &Window, _vibrancy: bool, _dark: bool) {}

pub fn apply_window_style(window: &Window, opacity: u8, dark: bool, cx: &mut App) {
    // `gpui-component`'s RootPlugin paints an opaque `theme.tokens.background`
    // on the Base `Root` surface unless overridden on `Root`'s own style.
    // Clear it to transparent so `Workspace`'s translucent surfaces and the
    // native macOS blur/transparency layers beneath can actually show through.
    if let Some(Some(root)) = window.root::<Root>() {
        root.update(cx, |root, cx| {
            root.style().background = Some(transparent_black().into());
            cx.notify();
        });
    }
    let blurred = opacity < 100;
    window.set_background_appearance(window_background_appearance(opacity));
    sync_macos_window_vibrancy(window, blurred, dark);
    hide_traffic_lights(window);
}

#[cfg(target_os = "macos")]
pub fn set_app_icon() {
    // When running from a packaged .app bundle, macOS already loads Portus.icns
    // from Info.plist. Avoid decoding and pinning an extra NSImage in memory.
    if std::env::current_exe()
        .ok()
        .is_some_and(|p| p.to_string_lossy().contains(".app/Contents/MacOS/"))
    {
        return;
    }

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

#[cfg(target_os = "macos")]
pub fn system_beep() {
    unsafe extern "C" {
        fn NSBeep();
    }
    unsafe { NSBeep() };
}

#[cfg(not(target_os = "macos"))]
pub fn system_beep() {}

/// Returns whether Portus is currently the active (foreground) OS application.
#[cfg(target_os = "macos")]
pub fn is_app_active() -> bool {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;

    let Some(mtm) = MainThreadMarker::new() else {
        return true;
    };
    NSApplication::sharedApplication(mtm).isActive()
}

#[cfg(not(target_os = "macos"))]
pub fn is_app_active() -> bool {
    true
}

/// Sends a non-blocking macOS desktop notification via `osascript` so it works
/// reliably in both `.app` bundles and unsigned `cargo run` dev sessions.
#[cfg(target_os = "macos")]
pub fn send_os_notification(title: &str, body: &str) {
    fn escape_applescript(input: &str) -> String {
        input
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace(['\r', '\n'], " ")
    }
    let title = escape_applescript(title.trim());
    let body = escape_applescript(body.trim());
    if body.is_empty() && title.is_empty() {
        return;
    }
    let script = if title.is_empty() {
        format!("display notification \"{body}\" with title \"Portus\"")
    } else {
        format!("display notification \"{body}\" with title \"{title}\"")
    };
    std::thread::spawn(move || {
        let _ = std::process::Command::new("osascript")
            .arg("-e")
            .arg(script)
            .status();
    });
}

#[cfg(not(target_os = "macos"))]
pub fn send_os_notification(_title: &str, _body: &str) {}

/// Opens an `http://`, `https://`, or `mailto:` URL in the user's default app.
pub fn open_external_url(url: &str) -> bool {
    let trimmed = url.trim();
    let allowed = trimmed.starts_with("https://")
        || trimmed.starts_with("http://")
        || trimmed.starts_with("mailto:");
    if !allowed {
        return false;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(trimmed)
            .spawn()
            .is_ok()
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::process::Command::new("xdg-open")
            .arg(trimmed)
            .spawn()
            .is_ok()
    }
}

/// Reads an image (PNG or TIFF converted to PNG) from the macOS system
/// pasteboard, returning raw PNG bytes if present.
#[cfg(target_os = "macos")]
pub fn read_clipboard_image_png() -> Option<Vec<u8>> {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};

    const NS_BITMAP_IMAGE_FILE_TYPE_PNG: usize = 4;

    unsafe {
        let pb_cls = AnyClass::get(c"NSPasteboard")?;
        let ns_string_cls = AnyClass::get(c"NSString")?;
        let pb: *mut AnyObject = msg_send![pb_cls, generalPasteboard];
        if pb.is_null() {
            return None;
        }

        let png_type: *mut AnyObject =
            msg_send![ns_string_cls, stringWithUTF8String: c"public.png".as_ptr()];
        let png_data: *mut AnyObject = if !png_type.is_null() {
            msg_send![pb, dataForType: png_type]
        } else {
            std::ptr::null_mut()
        };
        if !png_data.is_null() {
            return nsdata_to_vec(png_data);
        }

        let tiff_type: *mut AnyObject =
            msg_send![ns_string_cls, stringWithUTF8String: c"public.tiff".as_ptr()];
        let tiff_data: *mut AnyObject = if !tiff_type.is_null() {
            msg_send![pb, dataForType: tiff_type]
        } else {
            std::ptr::null_mut()
        };
        if tiff_data.is_null() {
            return None;
        }

        let rep_cls = AnyClass::get(c"NSBitmapImageRep")?;
        let dict_cls = AnyClass::get(c"NSDictionary")?;
        let rep: *mut AnyObject = msg_send![rep_cls, imageRepWithData: tiff_data];
        if rep.is_null() {
            return None;
        }
        let empty_dict: *mut AnyObject = msg_send![dict_cls, dictionary];
        let converted: *mut AnyObject = msg_send![
            rep,
            representationUsingType: NS_BITMAP_IMAGE_FILE_TYPE_PNG,
            properties: empty_dict
        ];
        if converted.is_null() {
            return None;
        }
        nsdata_to_vec(converted)
    }
}

#[cfg(target_os = "macos")]
unsafe fn nsdata_to_vec(data: *mut objc2::runtime::AnyObject) -> Option<Vec<u8>> {
    use objc2::msg_send;
    let len: usize = unsafe { msg_send![data, length] };
    if len == 0 {
        return None;
    }
    let ptr: *const u8 = unsafe { msg_send![data, bytes] };
    if ptr.is_null() {
        return None;
    }
    let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
    Some(slice.to_vec())
}

#[cfg(not(target_os = "macos"))]
pub fn read_clipboard_image_png() -> Option<Vec<u8>> {
    None
}
