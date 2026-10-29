//! Runs plug-in editor file descriptors and timers on GTK's main loop.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Duration;

use glib::{ControlFlow, IOCondition, SourceId};

thread_local! {
    static SOURCES: RefCell<HashMap<u64, SourceId>> = RefCell::new(HashMap::new());
    static NEXT: RefCell<u64> = const { RefCell::new(1) };
}

fn next_id() -> u64 {
    NEXT.with(|n| {
        let mut n = n.borrow_mut();
        *n += 1;
        *n
    })
}

pub struct GlibLoop;

impl vst3_host::editor::EventLoop for GlibLoop {
    fn add_fd(&self, fd: i32, mut cb: Box<dyn FnMut()>) -> u64 {
        let id = next_id();
        let src = glib::source::unix_fd_add_local(fd, IOCondition::IN | IOCondition::PRI, move |_, _| {
            cb();
            ControlFlow::Continue
        });
        SOURCES.with(|s| s.borrow_mut().insert(id, src));
        id
    }

    fn add_timer(&self, interval_ms: u64, mut cb: Box<dyn FnMut()>) -> u64 {
        let id = next_id();
        let src = glib::timeout_add_local(Duration::from_millis(interval_ms), move || {
            cb();
            ControlFlow::Continue
        });
        SOURCES.with(|s| s.borrow_mut().insert(id, src));
        id
    }

    fn remove(&self, id: u64) {
        if let Some(src) = SOURCES.with(|s| s.borrow_mut().remove(&id)) {
            src.remove();
        }
    }
}
