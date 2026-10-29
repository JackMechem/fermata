//! Runs plug-in editor file descriptors and timers on GTK's main loop.
//!
//! Main thread only: sources are added with glib's `*_local` functions, which need the default
//! main context's owner (they panic elsewhere), and their callbacks touch the editor, which
//! lives there. The id table is shared rather than thread-local all the same, so an editor
//! dropped on another thread still finds and removes its sources instead of leaving them to
//! call into freed memory.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use glib::{ControlFlow, IOCondition, SourceId};

static SOURCES: Mutex<Option<HashMap<u64, SourceId>>> = Mutex::new(None);
static NEXT: AtomicU64 = AtomicU64::new(1);

fn keep(src: SourceId) -> u64 {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    SOURCES.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(id, src);
    id
}

pub struct GlibLoop;

impl vst3_host::editor::EventLoop for GlibLoop {
    fn add_fd(&self, fd: i32, mut cb: Box<dyn FnMut()>) -> u64 {
        let src = glib::source::unix_fd_add_local(fd, IOCondition::IN | IOCondition::PRI, move |_, _| {
            cb();
            ControlFlow::Continue
        });
        keep(src)
    }

    fn add_timer(&self, interval_ms: u64, mut cb: Box<dyn FnMut()>) -> u64 {
        let src = glib::timeout_add_local(Duration::from_millis(interval_ms), move || {
            cb();
            ControlFlow::Continue
        });
        keep(src)
    }

    fn remove(&self, id: u64) {
        let src = SOURCES.lock().unwrap_or_else(|e| e.into_inner()).as_mut().and_then(|m| m.remove(&id));
        let Some(src) = src else { return };
        if !glib::MainContext::default().is_owner() {
            // GLib lets any thread destroy a source, so it won't be dispatched again, but a
            // callback already running on the main thread can't be stopped: editors must be
            // dropped on the main thread.
            log::error!("plug-in editor source {id} removed off the main thread");
        }
        src.remove();
    }
}
