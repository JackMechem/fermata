use vst3::Steinberg::char8;
use vst3::Steinberg::Vst::TChar;

/// Reads a NUL-terminated `char8` buffer into a `String`.
pub fn c_str(buf: &[char8]) -> String {
    let bytes: Vec<u8> = buf.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Reads a NUL-terminated UTF-16 buffer into a `String`.
#[allow(dead_code)]
pub fn utf16(buf: &[TChar]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

/// Reads a NUL-terminated UTF-16 string from a raw pointer.
pub unsafe fn utf16_ptr(ptr: *const TChar) -> Vec<u16> {
    let mut out = Vec::new();
    if ptr.is_null() {
        return out;
    }
    let mut i = 0;
    while *ptr.add(i) != 0 {
        out.push(*ptr.add(i));
        i += 1;
    }
    out
}

/// Copies `src` into a fixed-size UTF-16 buffer, always NUL-terminating.
pub fn copy_utf16(src: &str, dst: &mut [TChar]) {
    let mut n = 0;
    for (s, d) in src.encode_utf16().zip(dst.iter_mut()) {
        *d = s;
        n += 1;
    }
    let end = n.min(dst.len().saturating_sub(1));
    if !dst.is_empty() {
        dst[end] = 0;
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

pub fn unhex(s: &str) -> Option<[u8; 16]> {
    if s.len() != 32 {
        return None;
    }
    let mut out = [0u8; 16];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}
