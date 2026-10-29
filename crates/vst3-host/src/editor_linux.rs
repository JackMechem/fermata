//! Plugin editor windows on Linux: a top-level X11 window that the plugin embeds into, plus the
//! IRunLoop the plugin uses to register its file descriptors and timers.

use std::collections::HashMap;
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use parking_lot::Mutex;
use vst3::Steinberg::Linux::*;
use vst3::Steinberg::*;
use vst3::{Class, ComPtr, ComRef, ComWrapper};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::protocol::Event as XEvent;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

/// Integration point with the application's main loop (GTK/glib in the Tauri app). All methods
/// are called on the main thread, and callbacks must be invoked on the main thread.
pub trait EventLoop: Send + Sync {
    fn add_fd(&self, fd: i32, cb: Box<dyn FnMut()>) -> u64;
    fn add_timer(&self, interval_ms: u64, cb: Box<dyn FnMut()>) -> u64;
    fn remove(&self, id: u64);
}

static EVENT_LOOP: OnceLock<Box<dyn EventLoop>> = OnceLock::new();

pub fn set_event_loop(l: Box<dyn EventLoop>) {
    let _ = EVENT_LOOP.set(l);
}

fn event_loop() -> Option<&'static dyn EventLoop> {
    EVENT_LOOP.get().map(|b| b.as_ref())
}

struct Window {
    conn: RustConnection,
    id: u32,
    size: Mutex<(u32, u32)>,
    /// The view said it can be resized by the user; otherwise the WM hints pin its size.
    resizable: bool,
}

impl Window {
    /// WM_NORMAL_HINTS pinning a fixed-size editor to `w` x `h` (PMinSize | PMaxSize), plus
    /// PPosition when `at` gives the position.
    fn pin_size(&self, w: u32, h: u32, at: Option<(i16, i16)>) {
        let (pos, x, y) = match at {
            Some((x, y)) => (4, x as u32, y as u32),
            None => (0, 0, 0),
        };
        let hints: [u32; 18] = [pos | 16 | 32, x, y, w, h, w, h, w, h, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let _ = self.conn.change_property32(PropMode::REPLACE, self.id, AtomEnum::WM_NORMAL_HINTS, AtomEnum::WM_SIZE_HINTS, &hints);
    }
}

pub struct PlugFrame {
    window: Arc<Window>,
    view: Mutex<Option<ComPtr<IPlugView>>>,
    // handler pointer -> registered source ids
    sources: Mutex<HashMap<usize, Vec<u64>>>,
}

unsafe impl Send for PlugFrame {}
unsafe impl Sync for PlugFrame {}

impl Class for PlugFrame {
    type Interfaces = (IPlugFrame, IRunLoop);
}

impl PlugFrame {
    fn remove_all(&self) {
        if let Some(l) = event_loop() {
            for (_, ids) in self.sources.lock().drain() {
                for id in ids {
                    l.remove(id);
                }
            }
        }
    }
}

impl IPlugFrameTrait for PlugFrame {
    unsafe fn resizeView(&self, view: *mut IPlugView, new_size: *mut ViewRect) -> tresult {
        if new_size.is_null() {
            return kInvalidArgument;
        }
        let r = *new_size;
        let (w, h) = ((r.right - r.left).max(1) as u32, (r.bottom - r.top).max(1) as u32);
        *self.window.size.lock() = (w, h);
        // A fixed-size editor's hints hold it at its old size; move them along, or the WM
        // refuses the new one.
        if !self.window.resizable {
            self.window.pin_size(w, h, None);
        }
        let _ = self.window.conn.configure_window(self.window.id, &ConfigureWindowAux::new().width(w).height(h));
        let _ = self.window.conn.flush();
        if let Some(v) = ComRef::from_raw(view) {
            let mut r = r;
            v.onSize(&mut r);
        }
        kResultOk
    }
}

impl IRunLoopTrait for PlugFrame {
    unsafe fn registerEventHandler(&self, handler: *mut IEventHandler, fd: FileDescriptor) -> tresult {
        let (Some(l), Some(h)) = (event_loop(), ComRef::from_raw(handler)) else { return kResultFalse };
        let h: ComPtr<IEventHandler> = h.to_com_ptr();
        let id = l.add_fd(fd, Box::new(move || h.onFDIsSet(fd)));
        self.sources.lock().entry(handler as usize).or_default().push(id);
        kResultOk
    }

    unsafe fn unregisterEventHandler(&self, handler: *mut IEventHandler) -> tresult {
        if let (Some(l), Some(ids)) = (event_loop(), self.sources.lock().remove(&(handler as usize))) {
            for id in ids {
                l.remove(id);
            }
        }
        kResultOk
    }

    unsafe fn registerTimer(&self, handler: *mut ITimerHandler, ms: TimerInterval) -> tresult {
        let (Some(l), Some(h)) = (event_loop(), ComRef::from_raw(handler)) else { return kResultFalse };
        let h: ComPtr<ITimerHandler> = h.to_com_ptr();
        let id = l.add_timer(ms.max(1), Box::new(move || h.onTimer()));
        self.sources.lock().entry(handler as usize).or_default().push(id);
        kResultOk
    }

    unsafe fn unregisterTimer(&self, handler: *mut ITimerHandler) -> tresult {
        self.unregisterEventHandler(handler as *mut IEventHandler)
    }
}

/// Xlib's default error handler calls exit(), so a single bad request from a plugin GUI would
/// take the whole host down. Install a logging handler in every libX11 instance loaded into the
/// process (plugins may each bring their own copy, e.g. on NixOS).
fn install_x_error_handlers() {
    use std::ffi::{c_int, c_void, CStr};
    type Handler = unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int;
    unsafe extern "C" fn on_error(_dpy: *mut c_void, ev: *mut c_void) -> c_int {
        // XErrorEvent: type, display*, resourceid, serial, error_code(u8), request_code(u8), ...
        let bytes = ev as *const u8;
        let off = std::mem::size_of::<c_int>().max(8) + 3 * std::mem::size_of::<usize>();
        let (code, req) = (*bytes.add(off), *bytes.add(off + 1));
        log::warn!("X11 error from a plug-in GUI ignored (error {code}, request {req})");
        0
    }
    unsafe extern "C" fn visit(info: *mut libc::dl_phdr_info, _: usize, out: *mut c_void) -> c_int {
        let name = (*info).dlpi_name;
        if !name.is_null() {
            let s = CStr::from_ptr(name);
            if s.to_bytes().windows(9).any(|w| w == b"libX11.so") {
                (*(out as *mut Vec<std::ffi::CString>)).push(s.to_owned());
            }
        }
        0
    }
    let mut libs: Vec<std::ffi::CString> = Vec::new();
    unsafe {
        libc::dl_iterate_phdr(Some(visit), &mut libs as *mut _ as *mut c_void);
        for lib in libs {
            let h = libc::dlopen(lib.as_ptr(), libc::RTLD_NOW | libc::RTLD_NOLOAD);
            if h.is_null() {
                continue;
            }
            let f = libc::dlsym(h, b"XSetErrorHandler\0".as_ptr() as *const _);
            if !f.is_null() {
                let set: unsafe extern "C" fn(Option<Handler>) -> Option<Handler> = std::mem::transmute(f);
                set(Some(on_error));
            }
            libc::dlclose(h);
        }
    }
}

pub struct Editor {
    view: ComPtr<IPlugView>,
    frame: ComWrapper<PlugFrame>,
    window: Arc<Window>,
    wm_delete: u32,
    closed: Arc<AtomicBool>,
    x_source: Option<u64>,
    resizable: bool,
}

impl Editor {
    pub fn open(view: ComPtr<IPlugView>, title: &str) -> Result<Editor, String> {
        unsafe {
            if view.isPlatformTypeSupported(kPlatformTypeX11EmbedWindowID) != kResultOk {
                return Err("plugin editor does not support X11 embedding".into());
            }
            let mut rect: ViewRect = std::mem::zeroed();
            view.getSize(&mut rect);
            let w = ((rect.right - rect.left).max(64)) as u16;
            let h = ((rect.bottom - rect.top).max(64)) as u16;
            let resizable = view.canResize() == kResultTrue;

            let (conn, screen_num) = RustConnection::connect(None).map_err(|e| format!("X11: {e}"))?;
            let screen = conn.setup().roots[screen_num].clone();
            let id = conn.generate_id().map_err(|e| e.to_string())?;
            // Open near the middle of the screen rather than under panels in the corner.
            let x = (screen.width_in_pixels.saturating_sub(w) / 2) as i16;
            let y = (screen.height_in_pixels.saturating_sub(h) / 3) as i16;
            conn.create_window(
                x11rb::COPY_DEPTH_FROM_PARENT,
                id,
                screen.root,
                x,
                y,
                w,
                h,
                0,
                WindowClass::INPUT_OUTPUT,
                0,
                &CreateWindowAux::new()
                    .background_pixel(screen.black_pixel)
                    .event_mask(EventMask::STRUCTURE_NOTIFY),
            )
            .map_err(|e| e.to_string())?;

            let atom = |name: &str| conn.intern_atom(false, name.as_bytes()).ok().and_then(|c| c.reply().ok()).map(|r| r.atom).unwrap_or(0);
            let wm_protocols = atom("WM_PROTOCOLS");
            let wm_delete = atom("WM_DELETE_WINDOW");
            let net_wm_name = atom("_NET_WM_NAME");
            let utf8 = atom("UTF8_STRING");
            let _ = conn.change_property32(PropMode::REPLACE, id, wm_protocols, AtomEnum::ATOM, &[wm_delete]);
            let _ = conn.change_property8(PropMode::REPLACE, id, AtomEnum::WM_NAME, AtomEnum::STRING, title.as_bytes());
            let _ = conn.change_property8(PropMode::REPLACE, id, net_wm_name, utf8, title.as_bytes());
            let _ = conn.change_property8(PropMode::REPLACE, id, AtomEnum::WM_CLASS, AtomEnum::STRING, b"fermata-plugin\0Fermata\0");
            let window = Arc::new(Window { conn, id, size: Mutex::new((w as u32, h as u32)), resizable });
            if resizable {
                // WM_NORMAL_HINTS: just PPosition.
                let hints: [u32; 18] = [4, x as u32, y as u32, w as u32, h as u32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
                let _ = window.conn.change_property32(PropMode::REPLACE, id, AtomEnum::WM_NORMAL_HINTS, AtomEnum::WM_SIZE_HINTS, &hints);
            } else {
                window.pin_size(w as u32, h as u32, Some((x, y)));
            }
            window.conn.map_window(id).map_err(|e| e.to_string())?;
            window.conn.flush().map_err(|e| e.to_string())?;
            let frame = ComWrapper::new(PlugFrame {
                window: window.clone(),
                view: Mutex::new(Some(view.clone())),
                sources: Mutex::new(HashMap::new()),
            });
            view.setFrame(frame.to_com_ptr::<IPlugFrame>().unwrap().as_ptr());
            if view.attached(id as usize as *mut std::ffi::c_void, kPlatformTypeX11EmbedWindowID) != kResultOk {
                view.setFrame(std::ptr::null_mut());
                let _ = window.conn.destroy_window(id);
                let _ = window.conn.flush();
                return Err("plugin refused to attach its editor".into());
            }

            install_x_error_handlers();
            let closed = Arc::new(AtomicBool::new(false));
            let x_source = event_loop().map(|l| {
                let (win, closed) = (window.clone(), closed.clone());
                // A resizable view hears about the user's resizes from here too (onSize). It
                // stays alive with this source, which `close` removes before the window goes;
                // the callback can't run in between, as both are on the main thread.
                let sized = resizable.then(|| view.clone());
                let fd = window.conn.stream().as_raw_fd();
                l.add_fd(fd, Box::new(move || {
                    drain_events(&win, wm_delete, &closed, sized.as_ref());
                }))
            });

            Ok(Editor { view, frame, window, wm_delete, closed, x_source, resizable })
        }
    }

    /// Processes pending window events. Returns true once the user closed the window.
    pub fn pump(&mut self) -> bool {
        let view = if self.resizable { Some(&self.view) } else { None };
        drain_events(&self.window, self.wm_delete, &self.closed, view);
        self.closed.load(Ordering::Relaxed)
    }

    pub fn raise(&self) {
        let _ = self.window.conn.configure_window(self.window.id, &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE));
        let _ = self.window.conn.map_window(self.window.id);
        let _ = self.window.conn.flush();
    }

    /// `keep_alive` (the plug-in) is dropped after the view is removed.
    pub fn close<K: Send + 'static>(self, keep_alive: K) {
        unsafe {
            self.view.removed();
            self.view.setFrame(std::ptr::null_mut());
        }
        self.frame.remove_all();
        self.frame.view.lock().take();
        if let (Some(l), Some(id)) = (event_loop(), self.x_source) {
            l.remove(id);
        }
        let _ = self.window.conn.destroy_window(self.window.id);
        let _ = self.window.conn.flush();
        drop(keep_alive);
    }
}

fn drain_events(win: &Window, wm_delete: u32, closed: &AtomicBool, view: Option<&ComPtr<IPlugView>>) {
    while let Ok(Some(ev)) = win.conn.poll_for_event() {
        match ev {
            XEvent::ClientMessage(e) if e.data.as_data32()[0] == wm_delete => {
                closed.store(true, Ordering::Relaxed);
            }
            XEvent::ConfigureNotify(e) if e.window == win.id => {
                let (w, h) = (e.width as u32, e.height as u32);
                let mut size = win.size.lock();
                if *size != (w, h) {
                    *size = (w, h);
                    drop(size);
                    if let Some(v) = view {
                        let mut r = ViewRect { left: 0, top: 0, right: w as i32, bottom: h as i32 };
                        unsafe {
                            if v.checkSizeConstraint(&mut r) == kResultOk {
                                v.onSize(&mut r);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}
