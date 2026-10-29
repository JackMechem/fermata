//! Flushing denormals to zero on the audio thread.
//!
//! A decaying signal (a filter's tail, the end of a note, a DC blocker settling) ends up in the
//! denormal range, where x86 and many ARM cores do floating-point arithmetic about a hundred
//! times slower. A tuner or reverb chewing on such values can take a whole core and get the
//! audio thread killed for hogging it. Real-time audio code runs with flush-to-zero (and, on
//! x86, denormals-are-zero) on, which changes nothing audible: the values are below -700 dBFS.

/// Turns flush-to-zero on for this thread until dropped, then restores the previous mode
/// (the audio callback shares its thread with the driver's code).
pub struct NoDenormals {
    #[allow(dead_code)]
    saved: usize,
}

impl NoDenormals {
    #[inline]
    pub fn new() -> NoDenormals {
        NoDenormals { saved: unsafe { enable() } }
    }
}

impl Default for NoDenormals {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for NoDenormals {
    #[inline]
    fn drop(&mut self) {
        unsafe { restore(self.saved) }
    }
}

#[cfg(target_arch = "x86_64")]
unsafe fn enable() -> usize {
    // MXCSR bit 15 is flush-to-zero, bit 6 denormals-are-zero.
    let mut csr: u32 = 0;
    std::arch::asm!("stmxcsr [{}]", in(reg) &mut csr, options(nostack));
    let on = csr | 0x8040;
    std::arch::asm!("ldmxcsr [{}]", in(reg) &on, options(nostack));
    csr as usize
}

#[cfg(target_arch = "x86_64")]
unsafe fn restore(saved: usize) {
    let csr = saved as u32;
    std::arch::asm!("ldmxcsr [{}]", in(reg) &csr, options(nostack));
}

#[cfg(target_arch = "aarch64")]
unsafe fn enable() -> usize {
    // FPCR bit 24 is FZ (flush-to-zero).
    let fpcr: usize;
    std::arch::asm!("mrs {}, fpcr", out(reg) fpcr, options(nomem, nostack));
    std::arch::asm!("msr fpcr, {}", in(reg) fpcr | (1 << 24), options(nomem, nostack));
    fpcr
}

#[cfg(target_arch = "aarch64")]
unsafe fn restore(saved: usize) {
    std::arch::asm!("msr fpcr, {}", in(reg) saved, options(nomem, nostack));
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
unsafe fn enable() -> usize {
    0
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
unsafe fn restore(_saved: usize) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flushes_inside_the_guard_and_restores_after() {
        let tiny = std::hint::black_box(f32::MIN_POSITIVE);
        let half = std::hint::black_box(0.5f32);
        assert!(tiny * half > 0.0, "denormals are kept by default");
        {
            let _g = NoDenormals::new();
            if cfg!(any(target_arch = "x86_64", target_arch = "aarch64")) {
                assert_eq!(std::hint::black_box(tiny) * std::hint::black_box(half), 0.0);
            }
        }
        assert!(std::hint::black_box(tiny) * std::hint::black_box(half) > 0.0, "the mode is restored");
    }
}
