//! The page's host on macOS: a WKWebView inside an AppKit view laid over
//! Bevy's, which answers hit tests with nothing inside the page's hole, so
//! a click or a scroll there reaches Bevy's view (and makes it first
//! responder, so the keyboard follows) while the rest of the window is the
//! page's. The page itself comes from the binary, through the `lgui://`
//! scheme.

use std::borrow::Cow;
use std::cell::Cell;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::mpsc::Sender;

use bevy::prelude::Entity;
use bevy::winit::WINIT_WINDOWS;
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSAutoresizingMaskOptions, NSEventModifierFlags, NSMenu, NSMenuItem,
    NSResponder, NSView,
};
use objc2_foundation::{NSObject, NSPoint, NSRect, NSString};
use raw_window_handle::{
    AppKitWindowHandle, HandleError, HasWindowHandle, RawWindowHandle, WindowHandle,
};
use wry::http::{Response, header::CONTENT_TYPE};
use wry::{WebView, WebViewBuilder, WebViewExtMacOS};

/// The page over Bevy's view: what [`super::Host`] asks of a platform.
pub struct MacHost {
    webview: WebView,
    view: Retained<HoleView>,
}

impl super::Host for MacHost {
    fn run(&self, script: &str) {
        if let Err(e) = self.webview.evaluate_script(script) {
            tracing::warn!("interface: {e}");
        }
    }

    fn set_hole(&self, hole: [f64; 4]) {
        self.view.set_hole(hole);
    }
}

/// Bevy's view for `window`: winit's content view, once it has one.
pub fn content_view(window: Entity) -> Option<NonNull<c_void>> {
    WINIT_WINDOWS.with_borrow(|windows| {
        match windows.get_window(window)?.window_handle().ok()?.as_raw() {
            RawWindowHandle::AppKit(handle) => Some(handle.ns_view),
            _ => None,
        }
    })
}

/// Opens the page over `ns_view`: the built one, or `dev` (a dev server's
/// URL), with `query` after it. What the page posts arrives on `ipc`. Must
/// run on the main thread, after Bevy has made its surface.
pub fn open(
    ns_view: NonNull<c_void>,
    dev: Option<&str>,
    query: &str,
    ipc: Sender<String>,
) -> anyhow::Result<MacHost> {
    let url = format!("{}{query}", dev.unwrap_or("lgui://localhost/index.html"));
    let builder = WebViewBuilder::new()
        .with_transparent(true)
        .with_devtools(true)
        .with_accept_first_mouse(true)
        .with_custom_protocol("lgui".into(), |_, request| serve(request.uri().path()))
        .with_ipc_handler(move |request| {
            let _ = ipc.send(request.into_body());
        })
        .with_url(url);
    let (view, webview) = attach(ns_view, builder)?;
    add_edit_menu();
    Ok(MacHost { webview, view })
}

/// The built page: `npm run build` in `crates/app/ui`, read from disk in a
/// debug build and inside the binary in a release one. A build without it
/// still runs, and the window says how to build it.
#[derive(rust_embed::RustEmbed)]
#[folder = "ui/dist"]
#[allow_missing = true]
struct Dist;

/// The built page's files, by path; never above `dist`.
fn serve(path: &str) -> Response<Cow<'static, [u8]>> {
    let path = match path.trim_start_matches('/') {
        "" => "index.html",
        path => path,
    };
    let file = (!path.split('/').any(|part| part == ".."))
        .then(|| Dist::get(path))
        .flatten();
    match file {
        Some(file) => Response::builder()
            .header(CONTENT_TYPE, mime(path))
            .body(file.data),
        None if path == "index.html" => Response::builder()
            .header(CONTENT_TYPE, "text/html")
            .body(Cow::Borrowed(NOT_BUILT.as_bytes())),
        None => Response::builder()
            .status(404)
            .body(Cow::Borrowed(&b""[..])),
    }
    .expect("a response from static parts")
}

const NOT_BUILT: &str = "<body style='font:14px system-ui;color:#ddd;background:#111;padding:2em'>\
    The interface is not built: <code>npm ci &amp;&amp; npm run build</code> in \
    <code>crates/app/ui</code>, then build the app again — or set LOCALGPT_APP_UI_URL \
    to its dev server.</body>";

fn mime(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("html") => "text/html",
        Some("js" | "mjs") => "text/javascript",
        Some("css") => "text/css",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("woff2") => "font/woff2",
        Some("json") => "application/json",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
}

/// The hole, in the page's coordinates: CSS pixels from the top left.
#[derive(Default)]
pub struct Ivars {
    hole: Cell<[f64; 4]>,
}

define_class!(
    // SAFETY: NSView has no subclassing requirements this breaks, and
    // `HoleView` does not implement Drop.
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "LocalGPTHoleView"]
    #[ivars = Ivars]
    pub struct HoleView;

    impl HoleView {
        /// `point` is in the superview's coordinates, which are not flipped.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
            // SAFETY: a view's superview, read on the main thread.
            let superview = unsafe { self.superview() };
            let local = self.convertPoint_fromView(point, superview.as_deref());
            let [x, y, w, h] = self.ivars().hole.get();
            let top = self.bounds().size.height - local.y;
            // (No early return: the macro wraps this body.)
            if w > 0.0 && local.x >= x && local.x < x + w && top >= y && top < y + h {
                None
            } else {
                unsafe { msg_send![super(self), hitTest: point] }
            }
        }
    }
);

impl HoleView {
    fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars::default());
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }

    pub fn set_hole(&self, rect: [f64; 4]) {
        self.ivars().hole.set(rect);
    }
}

/// An AppKit view, as wry's `build_as_child` takes a parent.
struct Parent(NonNull<c_void>);

impl HasWindowHandle for Parent {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let raw = RawWindowHandle::AppKit(AppKitWindowHandle::new(self.0));
        // SAFETY: the view outlives the webview, which it holds as a subview.
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

/// Lays a [`HoleView`] over `ns_view` (Bevy's view, the window's content
/// view) and builds the webview inside it, both following the window's size.
/// Must run on the main thread, after Bevy has made its surface, so the
/// page sits above the Metal layer.
fn attach(
    ns_view: NonNull<c_void>,
    builder: WebViewBuilder<'_>,
) -> anyhow::Result<(Retained<HoleView>, WebView)> {
    let mtm = MainThreadMarker::new().ok_or_else(|| anyhow::anyhow!("not on the main thread"))?;
    // SAFETY: winit's view, alive for as long as the window is.
    let parent: &NSView = unsafe { ns_view.cast::<NSView>().as_ref() };
    let view = HoleView::new(mtm, parent.bounds());
    let fill =
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable;
    view.setAutoresizingMask(fill);
    parent.addSubview(&view);

    let handle = Parent(NonNull::from(&*view).cast());
    // wry's child default is 200 × 200; this one starts at the window's size.
    let size = parent.bounds().size;
    let webview = builder
        .with_bounds(wry::Rect {
            position: wry::dpi::LogicalPosition::new(0.0, 0.0).into(),
            size: wry::dpi::LogicalSize::new(size.width, size.height).into(),
        })
        .build_as_child(&handle)?;
    // wry pins a child's frame; this one fills the window as it resizes.
    webview.webview().setAutoresizingMask(fill);
    Ok((view, webview))
}

/// An Edit menu, so the standard keys reach the page: winit's menu has none,
/// and without it Cmd-C, Cmd-V and Cmd-A do nothing in a web view. The items
/// go to the first responder, so they are inert while Bevy's view has focus.
fn add_edit_menu() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    let Some(bar) = app.mainMenu() else {
        return;
    };
    let edit = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("Edit"));
    let items = [
        ("Undo", sel!(undo:), "z", NSEventModifierFlags::Command),
        (
            "Redo",
            sel!(redo:),
            "z",
            NSEventModifierFlags::Command | NSEventModifierFlags::Shift,
        ),
        ("Cut", sel!(cut:), "x", NSEventModifierFlags::Command),
        ("Copy", sel!(copy:), "c", NSEventModifierFlags::Command),
        ("Paste", sel!(paste:), "v", NSEventModifierFlags::Command),
        (
            "Select All",
            sel!(selectAll:),
            "a",
            NSEventModifierFlags::Command,
        ),
    ];
    for (i, (title, action, key, modifiers)) in items.into_iter().enumerate() {
        if i == 2 {
            edit.addItem(&NSMenuItem::separatorItem(mtm));
        }
        // SAFETY: standard responder actions, sent to the first responder.
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str(title),
                Some(action),
                &NSString::from_str(key),
            )
        };
        item.setKeyEquivalentModifierMask(modifiers);
        edit.addItem(&item);
    }
    let holder = NSMenuItem::new(mtm);
    holder.setSubmenu(Some(&edit));
    bar.addItem(&holder);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_is_served_but_never_above_its_folder() {
        let up = serve("/../Cargo.toml");
        assert_eq!(up.status(), 404);
        // With or without a built page, the index always answers.
        let index = serve("/");
        assert_eq!(index.status(), 200);
        assert_eq!(index.headers()[CONTENT_TYPE], "text/html");
        assert_eq!(mime("assets/index-abc.js"), "text/javascript");
    }
}
