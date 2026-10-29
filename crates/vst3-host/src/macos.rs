use std::ffi::c_void;
use std::path::{Path, PathBuf};

/// Creates a CFBundleRef for `bundleEntry`. The bundle is intentionally leaked: modules are never
/// unloaded.
pub fn create_bundle_ref(path: &Path) -> *mut c_void {
    use core_foundation::base::TCFType;
    use core_foundation::bundle::CFBundle;
    use core_foundation::url::CFURL;
    let Some(url) = CFURL::from_path(path, true) else { return std::ptr::null_mut() };
    let Some(bundle) = CFBundle::new(url) else { return std::ptr::null_mut() };
    let r = bundle.as_concrete_TypeRef();
    std::mem::forget(bundle);
    r as *mut c_void
}

/// The binary named by the bundle's `CFBundleExecutable`, when it exists.
pub fn bundle_executable(path: &Path) -> Option<PathBuf> {
    use core_foundation::bundle::CFBundle;
    use core_foundation::url::CFURL;
    let bundle = CFBundle::new(CFURL::from_path(path, true)?)?;
    let exe = bundle.executable_url()?.to_path()?;
    exe.is_file().then_some(exe)
}

/// The architectures a Mach-O file (thin or universal) holds code for, by Rust's names
/// ("aarch64", "x86_64"); `None` when it isn't a Mach-O file.
pub fn macho_archs(head: &[u8]) -> Option<Vec<&'static str>> {
    fn name(cpu: u32) -> &'static str {
        match cpu {
            0x0100_000c => "aarch64",
            0x0100_0007 => "x86_64",
            7 => "x86",
            12 => "arm",
            0x0100_0012 => "powerpc64",
            18 => "powerpc",
            _ => "unknown",
        }
    }
    let word = |i: usize, be: bool| -> Option<u32> {
        let b: [u8; 4] = head.get(i..i + 4)?.try_into().ok()?;
        Some(if be { u32::from_be_bytes(b) } else { u32::from_le_bytes(b) })
    };
    match word(0, true)? {
        // Universal binaries: a big-endian header, then one entry per architecture
        // (20 bytes each, or 32 in the 64-bit variant).
        m @ (0xcafe_babe | 0xcafe_babf) => {
            let n = word(4, true)? as usize;
            let size = if m == 0xcafe_babe { 20 } else { 32 };
            if n == 0 || n > 32 {
                return None;
            }
            (0..n).map(|i| word(8 + i * size, true).map(name)).collect()
        }
        // Thin binaries, in either byte order.
        0xcffa_edfe | 0xcefa_edfe => Some(vec![name(word(4, false)?)]),
        0xfeed_facf | 0xfeed_face => Some(vec![name(word(4, true)?)]),
        _ => None,
    }
}

/// Why `bin` can't be loaded when it has no code for this process's architecture, as a
/// message for the scan report; `None` when it has (or can't be read).
pub fn wrong_arch(bin: &Path) -> Option<String> {
    use std::io::Read;
    let mut head = vec![0u8; 8 + 32 * 32];
    let mut f = std::fs::File::open(bin).ok()?;
    let n = f.read(&mut head).ok()?;
    let archs = macho_archs(&head[..n])?;
    let ours = std::env::consts::ARCH;
    if archs.contains(&ours) {
        return None;
    }
    let mac = |a: &str| match a {
        "aarch64" => "Apple Silicon (arm64)".to_string(),
        "x86_64" => "Intel (x86_64)".to_string(),
        other => other.to_string(),
    };
    let built: Vec<String> = archs.iter().map(|a| mac(a)).collect();
    Some(format!(
        "built for {} only, and this copy of Fermata runs as {}: install a Universal or {} version of the plug-in",
        built.join(" and "),
        mac(ours),
        if ours == "aarch64" { "Apple Silicon" } else { "Intel" },
    ))
}

/// The first reason in a dyld error ("dlopen(<path>, 5): tried: '<path>' (<reason>), '<other
/// path>' (...)"), without the paths it went on to try.
pub fn dyld_reason(msg: &str) -> Option<String> {
    let rest = &msg[msg.find("tried: '")? + 8..];
    let rest = &rest[rest.find("' (")? + 3..];
    let mut depth = 0;
    for (i, c) in rest.char_indices() {
        match c {
            '(' => depth += 1,
            ')' if depth == 0 => return Some(rest[..i].to_string()),
            ')' => depth -= 1,
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{dyld_reason, macho_archs};

    #[test]
    fn reads_thin_and_universal_headers() {
        // A thin arm64 binary (little-endian 64-bit header).
        let thin = [0xcf, 0xfa, 0xed, 0xfe, 0x0c, 0x00, 0x00, 0x01, 0, 0, 0, 0];
        assert_eq!(macho_archs(&thin), Some(vec!["aarch64"]));
        // A universal binary with x86_64 and arm64 slices.
        let mut fat = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 2];
        for cpu in [0x0100_0007u32, 0x0100_000c] {
            fat.extend(cpu.to_be_bytes());
            fat.extend([0u8; 16]);
        }
        assert_eq!(macho_archs(&fat), Some(vec!["x86_64", "aarch64"]));
        assert_eq!(macho_archs(b"not a binary"), None);
        assert_eq!(macho_archs(&fat[..12]), None);
    }

    #[test]
    fn shortens_dyld_errors() {
        let msg = "dlopen failed: dlopen(/x/A.vst3/Contents/MacOS/A, 0x0005): tried: '/x/A.vst3/Contents/MacOS/A' (mach-o file, but is an incompatible architecture (have 'x86_64', need 'arm64')), '/System/Volumes/Preboot/Cryptexes/OS/x/A' (no such file)";
        assert_eq!(dyld_reason(msg).as_deref(), Some("mach-o file, but is an incompatible architecture (have 'x86_64', need 'arm64')"));
        assert_eq!(dyld_reason("dlopen failed"), None);
    }
}
