// The macOS implementation of the editor example: an NSApplication whose events are pumped by
// hand, with the controller's idle() between them.
use std::time::{Duration, Instant};

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSEventMask};
use vst3_host::editor::set_hide_on_deactivate;
use objc2_foundation::{NSDate, NSDefaultRunLoopMode};
use vst3_host::scan::{default_search_paths, find_bundles, scan_bundle};

pub fn main() {
    let name = std::env::args().nth(1).unwrap_or_else(|| "Surge XT".into());
    let secs: u64 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(4);
    let mtm = MainThreadMarker::new().expect("main thread");
    let app = NSApplication::sharedApplication(mtm);
    // A plain binary launched from a terminal needs a regular policy to show windows and take
    // focus.
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    app.finishLaunching();
    // macOS 14+ treats activation as a request a terminal-launched process may be denied, so
    // the window must not depend on the app becoming active.
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
    set_hide_on_deactivate(false);

    // An exact name wins over a substring ("Surge XT" vs "Surge XT Effects").
    let all: Vec<_> = find_bundles(&default_search_paths()).iter().flat_map(|b| scan_bundle(b).unwrap_or_default()).collect();
    let desc = all.iter().find(|d| d.name == name).or_else(|| all.iter().find(|d| d.name.contains(&name))).expect("plugin not found").clone();
    println!("opening {}", desc.name);
    let (_p, mut c) = vst3_host::instantiate(&desc, 48000.0, 512).unwrap();
    c.open_editor(&desc.name).unwrap();
    let start = Instant::now();
    let end = start + Duration::from_secs(secs);
    let mut reported = false;
    while Instant::now() < end && c.editor_open() {
        // Wait up to 5 ms for the first event (running the plug-in's run loop timers meanwhile),
        // then take whatever else is queued.
        let mut until = NSDate::dateWithTimeIntervalSinceNow(0.005);
        while let Some(ev) = unsafe { app.nextEventMatchingMask_untilDate_inMode_dequeue(NSEventMask::Any, Some(&until), NSDefaultRunLoopMode, true) } {
            app.sendEvent(&ev);
            until = NSDate::distantPast();
        }
        app.updateWindows();
        c.idle();
        // For headless runs: is the window really on screen?
        if !reported && start.elapsed() >= Duration::from_secs(1) {
            reported = true;
            println!("app active={}", app.isActive());
            for w in app.windows().iter() {
                let f = w.frame();
                println!(
                    "window {:?}: frame=({}, {}, {}x{}) visible={} level={} onActiveSpace={}",
                    w.title().to_string(),
                    f.origin.x,
                    f.origin.y,
                    f.size.width,
                    f.size.height,
                    w.isVisible(),
                    w.level(),
                    w.isOnActiveSpace()
                );
            }
        }
    }
    println!("editor ran for the test duration, open={}", c.editor_open());
    c.close_editor();
    println!("closed cleanly");
}
