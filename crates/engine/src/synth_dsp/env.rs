//! Envelopes with the curves of analog circuits: a capacitor charging through a resistor toward
//! a voltage it never quite reaches.
//!
//! - **Attack** charges toward a level above full ([`ATTACK_TARGET`]) and stops at full, so it
//!   curves over like a real contour's attack rather than running straight or, as a plain
//!   exponential to 1 would, never arriving.
//! - **Decay** and **release** discharge toward the sustain level and zero. Their times are how
//!   long it takes to fall 95 % of the way (three time constants).
//! - **Retriggering** doesn't jump: the attack carries on from wherever the level is, as it does
//!   in the circuit, so a note played over a decaying one doesn't click.
//!
//! The coefficients live in [`EnvShape`], one for each set of knobs, so that a change costs a few
//! `exp`s once rather than one per voice; each voice passes its own small tolerance to `tick`.

/// What the attack charges toward. A contour generator's RC charging toward a supply rail above
/// its peak level (a guess at a typical ratio): 1.3 gives a gently curved rise.
pub const ATTACK_TARGET: f32 = 1.3;

/// Below this (-100 dB) a released envelope is done.
pub const END_LEVEL: f32 = 1e-5;

/// The worked-out coefficients of one envelope's settings.
#[derive(Debug, Clone, Copy, Default)]
pub struct EnvShape {
    attack: f32,
    decay: f32,
    release: f32,
    sustain: f32,
}

/// The coefficient for a discharge falling 95 % of the way in `secs`.
fn fall(secs: f32, rate: f32) -> f32 {
    1.0 - (-3.0 / (secs.max(1e-5) * rate)).exp()
}

impl EnvShape {
    /// All times in seconds, `sustain` 0..1, `rate` what `tick` runs at.
    pub fn new(attack: f32, decay: f32, sustain: f32, release: f32, rate: f32) -> EnvShape {
        // Reaches 1 in `attack` seconds: 1 - e^(-t/tau) = 1/ATTACK_TARGET.
        let tau = attack.max(1e-5) / (ATTACK_TARGET / (ATTACK_TARGET - 1.0)).ln();
        EnvShape { attack: 1.0 - (-1.0 / (tau * rate)).exp(), decay: fall(decay, rate), release: fall(release, rate), sustain: sustain.clamp(0.0, 1.0) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EnvStage {
    #[default]
    Idle,
    Attack,
    Decay,
    Release,
}

/// One running envelope.
#[derive(Debug, Clone, Copy, Default)]
pub struct Envelope {
    pub level: f32,
    pub stage: EnvStage,
}

impl Envelope {
    /// Key down: the attack starts from the current level.
    #[inline]
    pub fn gate_on(&mut self) {
        self.stage = EnvStage::Attack;
    }

    /// Key up: the release starts from the current level.
    #[inline]
    pub fn gate_off(&mut self) {
        if self.stage != EnvStage::Idle {
            self.stage = EnvStage::Release;
        }
    }

    pub fn reset(&mut self) {
        *self = Envelope::default();
    }

    /// Whether it still produces anything.
    #[inline]
    pub fn active(&self) -> bool {
        self.stage != EnvStage::Idle
    }

    /// One sample. `speed` is the voice's tolerance, near 1: higher runs its times shorter.
    #[inline]
    pub fn tick(&mut self, s: &EnvShape, speed: f32) -> f32 {
        match self.stage {
            EnvStage::Idle => {}
            EnvStage::Attack => {
                self.level += (ATTACK_TARGET - self.level) * (s.attack * speed).min(1.0);
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = EnvStage::Decay;
                }
            }
            // The level follows the sustain knob too, as the circuit's capacitor does.
            EnvStage::Decay => self.level += (s.sustain - self.level) * (s.decay * speed).min(1.0),
            EnvStage::Release => {
                self.level -= self.level * (s.release * speed).min(1.0);
                if self.level < END_LEVEL {
                    self.level = 0.0;
                    self.stage = EnvStage::Idle;
                }
            }
        }
        self.level
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 96000.0;

    /// Seconds until `done` says so.
    fn time(e: &mut Envelope, s: &EnvShape, done: impl Fn(&Envelope) -> bool) -> f32 {
        let mut n = 0;
        while !done(e) && n < 40_000_000 {
            e.tick(s, 1.0);
            n += 1;
        }
        n as f32 / RATE
    }

    #[test]
    fn times_are_what_they_say() {
        for &t in &[0.001f32, 0.01, 0.1, 1.0, 10.0] {
            let s = EnvShape::new(t, t, 0.25, t, RATE);
            let mut e = Envelope::default();
            e.gate_on();
            let a = time(&mut e, &s, |e| e.stage == EnvStage::Decay);
            assert!((a / t - 1.0).abs() < 0.02, "attack {a} for {t}");
            // 95 % of the way from 1 to 0.25.
            let d = time(&mut e, &s, |e| e.level <= 0.25 + 0.05 * 0.75);
            assert!((d / t - 1.0).abs() < 0.02, "decay {d} for {t}");
            e.gate_off();
            let from = e.level;
            let r = time(&mut e, &s, |e| e.level <= 0.05 * from);
            assert!((r / t - 1.0).abs() < 0.03, "release {r} for {t}");
        }
    }

    #[test]
    fn attack_curves_over_and_decay_is_exponential() {
        let s = EnvShape::new(0.1, 0.1, 0.0, 0.1, RATE);
        let mut e = Envelope::default();
        e.gate_on();
        for _ in 0..(0.05 * RATE) as usize {
            e.tick(&s, 1.0);
        }
        // Half way through the time an RC charge is well past half way, and straight line isn't.
        assert!(e.level > 0.6 && e.level < 0.75, "{}", e.level);
        let mut seen = Vec::new();
        while e.stage == EnvStage::Attack {
            e.tick(&s, 1.0);
        }
        for _ in 0..4 {
            seen.push(e.level);
            for _ in 0..(0.02 * RATE) as usize {
                e.tick(&s, 1.0);
            }
        }
        // Equal times, equal ratios.
        let (r1, r2) = (seen[1] / seen[0], seen[3] / seen[2]);
        assert!((r1 - r2).abs() < 1e-3, "{r1} {r2}");
    }

    #[test]
    fn no_step_on_any_change_of_stage() {
        let s = EnvShape::new(0.02, 0.05, 0.5, 0.05, RATE);
        let mut e = Envelope::default();
        let mut last = 0.0f32;
        let mut biggest = 0.0f32;
        for i in 0..96000 {
            match i {
                1000 | 20000 | 40000 => e.gate_on(),
                1500 | 22000 | 60000 => e.gate_off(),
                _ => {}
            }
            let v = e.tick(&s, 1.0);
            biggest = biggest.max((v - last).abs());
            last = v;
        }
        // The steepest step is the attack's start: 1.3 times its coefficient.
        assert!(biggest < 1.31 * s.attack, "{biggest}");
    }

    #[test]
    fn reaches_idle_and_stays_there() {
        let s = EnvShape::new(0.001, 0.001, 1.0, 0.001, RATE);
        let mut e = Envelope::default();
        e.gate_on();
        for _ in 0..1000 {
            e.tick(&s, 1.0);
        }
        e.gate_off();
        for _ in 0..4000 {
            e.tick(&s, 1.0);
        }
        assert!(!e.active());
        assert_eq!(e.level, 0.0);
    }
}
