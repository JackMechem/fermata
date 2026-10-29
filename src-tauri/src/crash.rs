//! A last autosave when the app panics.
//!
//! A panic on the main thread ends the app: it unwinds into GTK's (or the webview's) C
//! callbacks, which aborts. The hook gets one chance before that. It only writes the autosave
//! when the session isn't locked: when the panic happened inside a session call (which holds
//! the lock), the project may be half-way through a change, and the lock can't be taken from
//! the same thread anyway. It never calls into plug-ins (their states come from the last
//! autosave or save; see `Session::emergency_autosave`). Panics on other threads don't end the
//! app (the audio callback even catches its own), so they're left to the normal autosave.
//! Crashes that aren't panics (a plug-in's segfault) can't be caught safely at all; the
//! regular autosave and the recording journal cover those.

use std::sync::Arc;

use daw_engine::Session;
use parking_lot::Mutex;

/// Chains a hook that tries an emergency autosave onto the default panic hook.
pub fn install(session: Arc<Mutex<Session>>) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        previous(info);
        if std::thread::current().name() != Some("main") {
            return;
        }
        let Some(s) = session.try_lock() else {
            log::error!("panic inside a session call; the last autosave is what can be recovered");
            return;
        };
        match s.emergency_autosave() {
            Ok(Some(p)) => log::error!("panic: wrote an emergency autosave to {}", p.display()),
            Ok(None) => {}
            Err(e) => log::error!("panic: the emergency autosave failed: {e}"),
        }
    }));
}
