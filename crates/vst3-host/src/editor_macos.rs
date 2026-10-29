//! Plugin editor windows on macOS: an NSWindow whose content view the plugin's NSView is added
//! to. Plug-ins drive their own timers through the main run loop, so unlike Linux there is no
//! IRunLoop. AppKit may only be touched on the main thread; `close` and `raise` hop there when
//! called from elsewhere. Keys the plug-in doesn't use go to the host (`keys::set_key_forwarder`).

use std::cell::Cell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};

use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSEvent, NSEventModifierFlags, NSFloatingWindowLevel, NSResponder, NSWindow, NSWindowCollectionBehavior,
    NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{NSNotification, NSPoint, NSRect, NSSize, NSString};
use vst3::Steinberg::*;
use vst3::{Class, ComPtr, ComRef, ComWrapper};

/// Moves a main-thread-only value through `exec_async` to the main thread, where it is used
/// and dropped.
struct MainOnly<T>(T);
unsafe impl<T> Send for MainOnly<T> {}

/// Runs `f` now on the main thread, or queues it there. Queued rather than waited for: the
/// caller may hold the session lock the main thread is waiting on.
fn on_main(f: impl FnOnce() + 'static) {
    if MainThreadMarker::new().is_some() {
        f();
    } else {
        let f = MainOnly(f);
        DispatchQueue::main().exec_async(move || {
            let f = f;
            (f.0)()
        });
    }
}

static HIDE_ON_DEACTIVATE: AtomicBool = AtomicBool::new(true);

/// Whether editor windows opened from now on hide while another app is active (the default).
/// The editor example turns it off: launched from a terminal, macOS may never make it the
/// active app, and its windows would then stay hidden.
pub fn set_hide_on_deactivate(on: bool) {
    HIDE_ON_DEACTIVATE.store(on, Ordering::Relaxed);
}

thread_local! {
    /// Where the next editor's top-left corner goes, so several editors don't stack exactly.
    static CASCADE: Cell<NSPoint> = const { Cell::new(NSPoint::new(0.0, 0.0)) };
    static OPEN: Cell<usize> = const { Cell::new(0) };
}

struct DelegateIvars {
    view: ComPtr<IPlugView>,
    resizable: bool,
    /// The content size (points) the view last heard about, from `resizeView` or a user resize.
    size: Cell<(f64, f64)>,
    /// The user closed the window (the red button only orders it out: not released on close).
    closed: Cell<bool>,
    /// `close` has started; late window callbacks leave the view alone.
    detached: Cell<bool>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and WindowDelegate doesn't implement Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FermataPlugInWindowDelegate"]
    #[ivars = DelegateIvars]
    struct WindowDelegate;

    unsafe impl NSObjectProtocol for WindowDelegate {}

    unsafe impl NSWindowDelegate for WindowDelegate {
        // A delegate rather than polling `isVisible`: hidesOnDeactivate also orders the window
        // out while another app is active.
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _n: &NSNotification) {
            self.ivars().closed.set(true);
        }

        /// Lets the plug-in constrain a user resize while it happens (`checkSizeConstraint`).
        #[unsafe(method(windowWillResize:toSize:))]
        fn window_will_resize(&self, sender: &NSWindow, frame_size: NSSize) -> NSSize {
            let iv = self.ivars();
            if iv.detached.get() || !iv.resizable {
                return frame_size;
            }
            let content = sender.contentRectForFrameRect(NSRect::new(NSPoint::new(0.0, 0.0), frame_size)).size;
            let mut r = ViewRect { left: 0, top: 0, right: content.width.round() as i32, bottom: content.height.round() as i32 };
            if unsafe { iv.view.checkSizeConstraint(&mut r) } != kResultOk {
                return frame_size;
            }
            let c = NSSize::new((r.right - r.left).max(1) as f64, (r.bottom - r.top).max(1) as f64);
            sender.frameRectForContentRect(NSRect::new(NSPoint::new(0.0, 0.0), c)).size
        }

        /// Tells the view about user resizes. `resizeView` records its own size first, so the
        /// resize it causes isn't passed back.
        #[unsafe(method(windowDidResize:))]
        fn window_did_resize(&self, n: &NSNotification) {
            let iv = self.ivars();
            if iv.detached.get() || !iv.resizable {
                return;
            }
            let Some(window) = n.object().and_then(|o| o.downcast::<NSWindow>().ok()) else { return };
            let s = window.contentRectForFrameRect(window.frame()).size;
            let size = (s.width.round(), s.height.round());
            if iv.size.get() == size {
                return;
            }
            iv.size.set(size);
            let mut r = ViewRect { left: 0, top: 0, right: size.0 as i32, bottom: size.1 as i32 };
            unsafe {
                iv.view.onSize(&mut r);
            }
        }
    }
);

impl WindowDelegate {
    fn new(mtm: MainThreadMarker, ivars: DelegateIvars) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ivars);
        unsafe { msg_send![super(this), init] }
    }
}

/// Resizes the window's content to `r`, keeping its top-left corner where it is (Cocoa frames
/// grow from the bottom-left), then calls `onSize`.
fn apply_resize(window: &NSWindow, delegate: &WindowDelegate, view: &ComPtr<IPlugView>, mut r: ViewRect) {
    if delegate.ivars().detached.get() {
        return;
    }
    let (w, h) = ((r.right - r.left).max(1) as f64, (r.bottom - r.top).max(1) as f64);
    delegate.ivars().size.set((w, h));
    let old = window.frame();
    let mut frame = window.frameRectForContentRect(NSRect::new(old.origin, NSSize::new(w, h)));
    frame.origin.y = old.origin.y + old.size.height - frame.size.height;
    window.setFrame_display(frame, true);
    unsafe {
        view.onSize(&mut r);
    }
}

pub struct PlugFrame {
    window: Retained<NSWindow>,
    delegate: Retained<WindowDelegate>,
}

impl Class for PlugFrame {
    type Interfaces = (IPlugFrame,);
}

impl IPlugFrameTrait for PlugFrame {
    unsafe fn resizeView(&self, view: *mut IPlugView, new_size: *mut ViewRect) -> tresult {
        if new_size.is_null() {
            return kInvalidArgument;
        }
        let Some(v) = ComRef::from_raw(view) else { return kInvalidArgument };
        let (r, v) = (*new_size, v.to_com_ptr());
        if MainThreadMarker::new().is_some() {
            apply_resize(&self.window, &self.delegate, &v, r);
        } else {
            let (w, d) = (self.window.clone(), self.delegate.clone());
            on_main(move || apply_resize(&w, &d, &v, r));
        }
        kResultOk
    }
}

pub struct Editor {
    view: ComPtr<IPlugView>,
    frame: ComWrapper<PlugFrame>,
    window: Retained<NSWindow>,
    delegate: Retained<WindowDelegate>,
}

impl Editor {
    /// Must be called on the main thread; elsewhere it fails rather than blocking on it.
    pub fn open(view: ComPtr<IPlugView>, title: &str) -> Result<Editor, String> {
        let mtm = MainThreadMarker::new().ok_or("plug-in editors can only be opened on the main thread")?;
        unsafe {
            if view.isPlatformTypeSupported(kPlatformTypeNSView) != kResultOk {
                return Err("this plug-in's editor does not support macOS (NSView) embedding".into());
            }
            // VST3 sizes on macOS are in points, the same units as AppKit's.
            let mut rect: ViewRect = std::mem::zeroed();
            view.getSize(&mut rect);
            let w = (rect.right - rect.left).max(64) as f64;
            let h = (rect.bottom - rect.top).max(64) as f64;
            let resizable = view.canResize() == kResultTrue;

            let mut style = NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::Miniaturizable;
            if resizable {
                style |= NSWindowStyleMask::Resizable;
            }
            let window = PlugInWindow::new(mtm, NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(w, h)), style);
            // We own the window through the Retained; the close button just orders it out.
            window.setReleasedWhenClosed(false);
            window.setTitle(&NSString::from_str(title));
            // Like Logic's plug-in windows: floating above the project windows, hidden while
            // another app is active so they don't cover it (switching between Fermata's own
            // windows keeps the app active, so that doesn't hide them), and shown over a full-screen
            // main window.
            window.setLevel(NSFloatingWindowLevel);
            window.setHidesOnDeactivate(HIDE_ON_DEACTIVATE.load(Ordering::Relaxed));
            window.setCollectionBehavior(NSWindowCollectionBehavior::FullScreenAuxiliary);
            // The first editor is centred; while it is open, the next ones cascade from it.
            window.center();
            let from = if OPEN.get() == 0 { NSPoint::new(0.0, 0.0) } else { CASCADE.get() };
            CASCADE.set(window.cascadeTopLeftFromPoint(from));

            let delegate = WindowDelegate::new(
                mtm,
                DelegateIvars {
                    view: view.clone(),
                    resizable,
                    size: Cell::new((w, h)),
                    closed: Cell::new(false),
                    detached: Cell::new(false),
                },
            );
            window.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            let frame = ComWrapper::new(PlugFrame { window: window.clone(), delegate: delegate.clone() });
            view.setFrame(frame.to_com_ptr::<IPlugFrame>().unwrap().as_ptr());
            let content = window.contentView().ok_or("editor window has no content view")?;
            if view.attached(Retained::as_ptr(&content) as *mut c_void, kPlatformTypeNSView) != kResultOk {
                view.setFrame(std::ptr::null_mut());
                delegate.ivars().detached.set(true);
                window.setDelegate(None);
                window.close();
                return Err("plugin refused to attach its editor".into());
            }
            window.makeKeyAndOrderFront(None);
            OPEN.set(OPEN.get() + 1);
            Ok(Editor { view, frame, window, delegate })
        }
    }

    /// Returns true once the user closed the window. Resizes are handled as they happen.
    pub fn pump(&mut self) -> bool {
        MainThreadMarker::new().is_some() && self.delegate.ivars().closed.get()
    }

    pub fn raise(&self) {
        let w = MainOnly(self.window.clone());
        on_main(move || {
            let w = w;
            if w.0.isMiniaturized() {
                w.0.deminiaturize(None);
            }
            w.0.makeKeyAndOrderFront(None);
        });
    }

    /// Closes the editor on the main thread (later, if called from another thread), then drops
    /// `keep_alive`: the plug-in mustn't be terminated before its view is removed.
    pub fn close<K: Send + 'static>(self, keep_alive: K) {
        let this = MainOnly(self);
        on_main(move || {
            let this = this;
            this.0.close_now();
            drop(keep_alive);
        });
    }

    fn close_now(self) {
        self.delegate.ivars().detached.set(true);
        unsafe {
            self.view.removed();
            self.view.setFrame(std::ptr::null_mut());
        }
        self.window.setDelegate(None);
        self.window.close();
        OPEN.set(OPEN.get().saturating_sub(1));
        drop(self.frame);
    }
}

// ---- keys the plug-in doesn't use -----------------------------------------------------------

define_class!(
    /// The editor window. A key the plug-in's view doesn't handle travels up the responder
    /// chain to here (its text fields keep theirs), and goes to the host's key forwarder rather
    /// than beeping. ⌘ shortcuts of the main menu never get here: the menu takes them first.
    // SAFETY: NSWindow may be subclassed; PlugInWindow has no ivars and doesn't implement Drop.
    #[unsafe(super(NSWindow, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FermataPlugInWindow"]
    struct PlugInWindow;

    impl PlugInWindow {
        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            if !forward_key(event, false) {
                unsafe {
                    let _: () = msg_send![super(self), keyDown: event];
                }
            }
        }

        /// Releases too, for Musical Typing.
        #[unsafe(method(keyUp:))]
        fn key_up(&self, event: &NSEvent) {
            if !forward_key(event, true) {
                unsafe {
                    let _: () = msg_send![super(self), keyUp: event];
                }
            }
        }
    }
);

impl PlugInWindow {
    fn new(mtm: MainThreadMarker, content: NSRect, style: NSWindowStyleMask) -> Retained<NSWindow> {
        let this = Self::alloc(mtm).set_ivars(());
        let this: Retained<Self> = unsafe {
            msg_send![super(this), initWithContentRect: content, styleMask: style, backing: NSBackingStoreType::Buffered, defer: false]
        };
        Retained::into_super(this)
    }
}

/// Sends a key event to the forwarder; false if there is none or the key has no name.
fn forward_key(event: &NSEvent, up: bool) -> bool {
    if !crate::keys::has_forwarder() {
        return false;
    }
    let chars = event.charactersIgnoringModifiers().map(|s| s.to_string()).unwrap_or_default();
    let Some((key, code)) = crate::keys::mac_key(event.keyCode(), &chars) else { return false };
    let flags = event.modifierFlags();
    crate::keys::forward(crate::keys::KeyInfo {
        up,
        key,
        code,
        shift_key: flags.contains(NSEventModifierFlags::Shift),
        ctrl_key: flags.contains(NSEventModifierFlags::Control),
        alt_key: flags.contains(NSEventModifierFlags::Option),
        meta_key: flags.contains(NSEventModifierFlags::Command),
        repeat: !up && event.isARepeat(),
    })
}
