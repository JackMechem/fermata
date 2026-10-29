// The Linux implementation of the editor example (X11 embedding).
use std::cell::RefCell;
use std::time::{Duration, Instant};

use vst3_host::editor::{set_event_loop, EventLoop};
use vst3_host::scan::{default_search_paths, find_bundles, scan_bundle};

enum Src {
    Fd(i32, Box<dyn FnMut()>),
    Timer(Duration, Instant, Box<dyn FnMut()>),
}

thread_local! {
    static SOURCES: RefCell<Vec<(u64, Option<Src>)>> = const { RefCell::new(Vec::new()) };
}
static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

struct PollLoop;
impl EventLoop for PollLoop {
    fn add_fd(&self, fd: i32, cb: Box<dyn FnMut()>) -> u64 {
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        SOURCES.with(|s| s.borrow_mut().push((id, Some(Src::Fd(fd, cb)))));
        id
    }
    fn add_timer(&self, ms: u64, cb: Box<dyn FnMut()>) -> u64 {
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let d = Duration::from_millis(ms);
        SOURCES.with(|s| s.borrow_mut().push((id, Some(Src::Timer(d, Instant::now() + d, cb)))));
        id
    }
    fn remove(&self, id: u64) {
        SOURCES.with(|s| s.borrow_mut().retain(|(i, _)| *i != id));
    }
}

fn run_once() {
    // Take sources out while running callbacks so they may add/remove sources.
    let ids: Vec<u64> = SOURCES.with(|s| s.borrow().iter().map(|(i, _)| *i).collect());
    for id in ids {
        let src = SOURCES.with(|s| s.borrow_mut().iter_mut().find(|(i, _)| *i == id).and_then(|(_, x)| x.take()));
        let Some(mut src) = src else { continue };
        match &mut src {
            Src::Fd(fd, cb) => {
                let mut p = libc_pollfd { fd: *fd, events: 1, revents: 0 };
                if unsafe { poll(&mut p, 1, 0) } > 0 {
                    cb();
                }
            }
            Src::Timer(d, next, cb) => {
                if Instant::now() >= *next {
                    *next = Instant::now() + *d;
                    cb();
                }
            }
        }
        SOURCES.with(|s| {
            if let Some((_, slot)) = s.borrow_mut().iter_mut().find(|(i, _)| *i == id) {
                *slot = Some(src);
            }
        });
    }
}

#[repr(C)]
struct libc_pollfd {
    fd: i32,
    events: i16,
    revents: i16,
}
extern "C" {
    fn poll(fds: *mut libc_pollfd, n: u64, timeout: i32) -> i32;
}

pub fn main() {
    let name = std::env::args().nth(1).unwrap_or_else(|| "ZamComp".into());
    let secs: u64 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(4);
    set_event_loop(Box::new(PollLoop));
    // An exact name wins over a substring ("Surge XT" vs "Surge XT Effects").
    let all: Vec<_> = find_bundles(&default_search_paths()).iter().flat_map(|b| scan_bundle(b).unwrap_or_default()).collect();
    let desc = all.iter().find(|d| d.name == name).or_else(|| all.iter().find(|d| d.name.contains(&name))).expect("plugin not found").clone();
    println!("opening {}", desc.name);
    let (_p, mut c) = vst3_host::instantiate(&desc, 48000.0, 512).unwrap();
    c.open_editor(&desc.name).unwrap();
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end && c.editor_open() {
        run_once();
        c.idle();
        std::thread::sleep(Duration::from_millis(5));
    }
    println!("editor ran for the test duration, open={}", c.editor_open());
    c.close_editor();
    println!("closed cleanly");
}
