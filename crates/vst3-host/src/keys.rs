//! Keys a plug-in editor doesn't use, passed on to the host app, as Logic runs its key commands
//! while a plug-in window is in front. The host installs a forwarder once; editor windows call
//! it on the main thread with keys that went up the responder chain unhandled (macOS). The key
//! is described the way a browser's KeyboardEvent would, so a webview UI can treat it as its own.

use parking_lot::RwLock;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyInfo {
    /// A key release; otherwise a press.
    pub up: bool,
    /// KeyboardEvent.key: the character ("r", "R", ",", " ") or the key's name ("Enter").
    pub key: String,
    /// KeyboardEvent.code: the physical key ("KeyR", "Space"), or "" if it has no name.
    pub code: String,
    pub shift_key: bool,
    /// The Control key (on macOS, not ⌘).
    pub ctrl_key: bool,
    pub alt_key: bool,
    /// ⌘ on macOS.
    pub meta_key: bool,
    pub repeat: bool,
}

type Forwarder = Box<dyn Fn(KeyInfo) + Send + Sync>;

static FORWARDER: RwLock<Option<Forwarder>> = parking_lot::const_rwlock(None);

/// Sets where editor windows send the keys their plug-in didn't use. Without one, they get the
/// platform's default handling (a beep on macOS).
pub fn set_key_forwarder(f: Forwarder) {
    *FORWARDER.write() = Some(f);
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn has_forwarder() -> bool {
    FORWARDER.read().is_some()
}

/// Passes `k` to the forwarder; false if there is none.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn forward(k: KeyInfo) -> bool {
    match &*FORWARDER.read() {
        Some(f) => {
            f(k);
            true
        }
        None => false,
    }
}

/// The KeyboardEvent `key` and `code` of a macOS virtual key code (kVK_*, which name physical
/// positions like `code` does) and its `charactersIgnoringModifiers`. None for keys a browser
/// couldn't name either (a dead key, an unknown function key).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn mac_key(key_code: u16, chars: &str) -> Option<(String, String)> {
    // Keys whose characters are control codes or private-use function-key characters.
    let named = match key_code {
        36 => Some(("Enter", "Enter")),
        76 => Some(("Enter", "NumpadEnter")),
        48 => Some(("Tab", "Tab")),
        49 => Some((" ", "Space")),
        51 => Some(("Backspace", "Backspace")),
        117 => Some(("Delete", "Delete")),
        53 => Some(("Escape", "Escape")),
        123 => Some(("ArrowLeft", "ArrowLeft")),
        124 => Some(("ArrowRight", "ArrowRight")),
        125 => Some(("ArrowDown", "ArrowDown")),
        126 => Some(("ArrowUp", "ArrowUp")),
        115 => Some(("Home", "Home")),
        119 => Some(("End", "End")),
        116 => Some(("PageUp", "PageUp")),
        121 => Some(("PageDown", "PageDown")),
        114 => Some(("Help", "Help")),
        71 => Some(("Clear", "NumLock")),
        122 => Some(("F1", "F1")),
        120 => Some(("F2", "F2")),
        99 => Some(("F3", "F3")),
        118 => Some(("F4", "F4")),
        96 => Some(("F5", "F5")),
        97 => Some(("F6", "F6")),
        98 => Some(("F7", "F7")),
        100 => Some(("F8", "F8")),
        101 => Some(("F9", "F9")),
        109 => Some(("F10", "F10")),
        103 => Some(("F11", "F11")),
        111 => Some(("F12", "F12")),
        105 => Some(("F13", "F13")),
        107 => Some(("F14", "F14")),
        113 => Some(("F15", "F15")),
        106 => Some(("F16", "F16")),
        64 => Some(("F17", "F17")),
        79 => Some(("F18", "F18")),
        80 => Some(("F19", "F19")),
        90 => Some(("F20", "F20")),
        _ => None,
    };
    if let Some((key, code)) = named {
        return Some((key.into(), code.into()));
    }
    let code = match key_code {
        0 => "KeyA",
        11 => "KeyB",
        8 => "KeyC",
        2 => "KeyD",
        14 => "KeyE",
        3 => "KeyF",
        5 => "KeyG",
        4 => "KeyH",
        34 => "KeyI",
        38 => "KeyJ",
        40 => "KeyK",
        37 => "KeyL",
        46 => "KeyM",
        45 => "KeyN",
        31 => "KeyO",
        35 => "KeyP",
        12 => "KeyQ",
        15 => "KeyR",
        1 => "KeyS",
        17 => "KeyT",
        32 => "KeyU",
        9 => "KeyV",
        13 => "KeyW",
        7 => "KeyX",
        16 => "KeyY",
        6 => "KeyZ",
        29 => "Digit0",
        18 => "Digit1",
        19 => "Digit2",
        20 => "Digit3",
        21 => "Digit4",
        23 => "Digit5",
        22 => "Digit6",
        26 => "Digit7",
        28 => "Digit8",
        25 => "Digit9",
        27 => "Minus",
        24 => "Equal",
        33 => "BracketLeft",
        30 => "BracketRight",
        42 => "Backslash",
        41 => "Semicolon",
        39 => "Quote",
        50 => "Backquote",
        43 => "Comma",
        47 => "Period",
        44 => "Slash",
        10 => "IntlBackslash",
        82 => "Numpad0",
        83 => "Numpad1",
        84 => "Numpad2",
        85 => "Numpad3",
        86 => "Numpad4",
        87 => "Numpad5",
        88 => "Numpad6",
        89 => "Numpad7",
        91 => "Numpad8",
        92 => "Numpad9",
        65 => "NumpadDecimal",
        67 => "NumpadMultiply",
        69 => "NumpadAdd",
        75 => "NumpadDivide",
        78 => "NumpadSubtract",
        81 => "NumpadEqual",
        _ => "",
    };
    // One printable character, as the keyboard layout produces it (Shift included).
    let mut it = chars.chars();
    let c = it.next()?;
    if it.next().is_some() || c.is_control() || ('\u{e000}'..='\u{f8ff}').contains(&c) {
        return None;
    }
    Some((c.to_string(), code.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is(code: u16, chars: &str, want: Option<(&str, &str)>) {
        assert_eq!(mac_key(code, chars).as_ref().map(|(k, c)| (k.as_str(), c.as_str())), want, "key code {code}");
    }

    #[test]
    fn names_keys_as_keyboard_events_do() {
        is(49, " ", Some((" ", "Space")));
        is(15, "r", Some(("r", "KeyR")));
        is(15, "R", Some(("R", "KeyR")));
        is(43, ",", Some((",", "Comma")));
        // Shift is part of the character, as in a browser.
        is(43, "<", Some(("<", "Comma")));
        is(50, "`", Some(("`", "Backquote")));
        is(18, "1", Some(("1", "Digit1")));
        is(83, "1", Some(("1", "Numpad1")));
        is(36, "\r", Some(("Enter", "Enter")));
        is(76, "\u{3}", Some(("Enter", "NumpadEnter")));
        is(51, "\u{7f}", Some(("Backspace", "Backspace")));
        is(117, "\u{f728}", Some(("Delete", "Delete")));
        is(53, "\u{1b}", Some(("Escape", "Escape")));
        is(123, "\u{f702}", Some(("ArrowLeft", "ArrowLeft")));
        is(126, "\u{f700}", Some(("ArrowUp", "ArrowUp")));
        is(122, "\u{f704}", Some(("F1", "F1")));
        is(111, "\u{f70f}", Some(("F12", "F12")));
        // Another layout: the character follows it, the code stays the physical key.
        is(16, "z", Some(("z", "KeyY")));
        // A key with no code name still carries its character.
        is(93, "¥", Some(("¥", "")));
    }

    #[test]
    fn skips_keys_without_a_name() {
        is(14, "", None);
        is(200, "\u{f71a}", None);
        is(200, "\u{1}", None);
    }
}
