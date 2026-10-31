//! A sampler voice: one zone's sample through the interpolator, a filter and the envelopes. A key
//! can sound several of these, one per layered zone.

use std::sync::Arc;

use super::kernel::{kernels, Bank, Kernels, GUARD, MAX_TAPS};
use super::map::SamplerMap;
use crate::synth_dsp::env::{EnvShape, EnvStage, Envelope, END_LEVEL};
use crate::synth_dsp::filter::{cutoff_g, Ladder, ZdfFilter};
use crate::synth_dsp::smooth::Glide;
use crate::synth_dsp::pow2;

/// A stolen voice's fade. Long enough not to click, short enough to stay out of the way.
pub const STEAL_FADE_SECS: f32 = 0.004;
/// A sample that starts away from zero fades in over this many output samples.
const START_RAMP: f32 = 32.0;
/// The last output samples of a sample's end fade out, so an end trimmed mid-wave doesn't click.
const END_FADE: f64 = 64.0;

/// The filter's choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FilterKind {
    #[default]
    Off,
    Low12,
    Band12,
    High12,
    Ladder24,
}

/// A 2-pole state-variable filter, clean, with its coefficients safe to change while it runs
/// (Simper's trapezoidal form).
#[derive(Debug, Clone, Copy, Default)]
struct Svf {
    ic1: f32,
    ic2: f32,
}

#[derive(Debug, Clone, Copy, Default)]
struct SvfCoef {
    a1: f32,
    a2: f32,
    a3: f32,
    k: f32,
}

impl SvfCoef {
    fn new(g: f32, k: f32) -> SvfCoef {
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        SvfCoef { a1, a2, a3: g * a2, k }
    }
}

impl Svf {
    #[inline]
    fn tick(&mut self, c: &SvfCoef, kind: FilterKind, x: f32) -> f32 {
        let v3 = x - self.ic2;
        let v1 = c.a1 * self.ic1 + c.a2 * v3;
        let v2 = self.ic2 + c.a2 * self.ic1 + c.a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        if !(self.ic1.abs() < 1e6 && self.ic2.abs() < 1e6) {
            self.ic1 = 0.0;
            self.ic2 = 0.0;
        }
        match kind {
            FilterKind::Low12 => v2,
            FilterKind::Band12 => v1,
            _ => x - c.k * v1 - v2,
        }
    }
}

/// What every voice shares, worked out at the control rate.
#[derive(Debug, Clone, Copy, Default)]
pub struct Control {
    pub rate: f32,
    /// Added to every voice's note: transpose, tune and bend, in semitones.
    pub tune_st: f64,
    pub vel_amp: f32,
    pub amp: EnvShape,
    pub filter_env: EnvShape,
    pub filter: FilterKind,
    pub cutoff_note: f32,
    /// 0..1.
    pub resonance: f32,
    pub key_track: f32,
    pub env_st: f32,
    pub vel_cutoff_st: f32,
    /// This control tick's LFO in semitones, at the pitch and at the filter.
    pub lfo_pitch_st: f32,
    pub lfo_filter_st: f32,
    pub glide_secs: f32,
    pub control_rate: f32,
}

/// The cutoff as a note to Hz.
#[inline]
fn note_to_hz(note: f32) -> f32 {
    440.0 * pow2((note.clamp(-24.0, 140.0) - 69.0) / 12.0)
}

/// The note of a frequency.
pub fn hz_to_note(hz: f32) -> f32 {
    69.0 + 12.0 * (hz.max(1.0) / 440.0).log2()
}

pub struct Voice {
    pub active: bool,
    /// The allocator's voice this one sounds for; None once stolen.
    pub owner: Option<u8>,
    /// When it started; lower is older.
    pub age: u64,
    map: Option<Arc<SamplerMap>>,
    zone: u32,
    pos: f64,
    in_loop: bool,
    /// Past this frame the voice reads the baked loop, whose audio matches the sample here.
    enter_loop_at: f64,
    inc: f64,
    bank: u32,
    end_fade: f64,
    glide: Glide,
    velocity: f32,
    vel_gain: f32,
    amp: Envelope,
    filter_env: Envelope,
    /// The zone's own amp envelope, or the instrument's.
    own_env: Option<EnvShape>,
    released: bool,
    one_shot: bool,
    /// 1 until stolen, then falling to 0 at `kill_step` a sample.
    kill: f32,
    kill_step: f32,
    ramp: f32,
    ladder: [Ladder; 2],
    svf: [Svf; 2],
    svf_coef: SvfCoef,
}

impl Default for Voice {
    fn default() -> Voice {
        Voice {
            active: false,
            owner: None,
            age: 0,
            map: None,
            zone: 0,
            pos: 0.0,
            in_loop: false,
            enter_loop_at: f64::MAX,
            inc: 1.0,
            bank: 0,
            end_fade: END_FADE,
            glide: Glide::default(),
            velocity: 0.0,
            vel_gain: 1.0,
            amp: Envelope::default(),
            filter_env: Envelope::default(),
            own_env: None,
            released: false,
            one_shot: false,
            kill: 1.0,
            kill_step: 0.0,
            ramp: 1.0,
            ladder: [Ladder::default(); 2],
            svf: [Svf::default(); 2],
            svf_coef: SvfCoef::default(),
        }
    }
}

impl Voice {
    /// Stops dead. Lets go of its map, which can't be the last hold on it: the instance or the
    /// session has one until the voices are done.
    pub fn clear(&mut self) {
        self.active = false;
        self.owner = None;
        self.map = None;
    }

    #[inline]
    pub fn fading(&self) -> bool {
        self.kill_step > 0.0
    }

    /// Whether a voice with this much of its fade left is the quietest choice to cut.
    #[inline]
    pub fn kill_level(&self) -> f32 {
        self.kill
    }

    /// Starts `zone` of `map` on `note`.
    #[allow(clippy::too_many_arguments)]
    pub fn start(&mut self, map: &Arc<SamplerMap>, zone: u32, owner: u8, age: u64, note: f32, from_note: Option<f32>, velocity: f32, ctl: &Control) {
        let z = &map.zones[zone as usize];
        let start = z.start;
        let (hard, one_shot, own_env) = (z.hard_start, z.one_shot, z.env);
        let enter = z.looped.as_ref().map(|l| {
            let half = kernels().bank(kernels().bank_index(z.base_inc.max(1.0))).taps / 2;
            z.loop_start + half.min(l.len.saturating_sub(1)) as f64
        });
        self.active = true;
        self.owner = Some(owner);
        self.age = age;
        self.map = Some(map.clone());
        self.zone = zone;
        self.pos = start;
        self.in_loop = false;
        self.enter_loop_at = enter.unwrap_or(f64::MAX);
        self.glide.set_time(ctl.glide_secs, ctl.control_rate);
        self.glide.jump(from_note.unwrap_or(note));
        self.glide.target = note;
        self.velocity = velocity;
        self.vel_gain = (1.0 - ctl.vel_amp) + ctl.vel_amp * velocity * velocity;
        self.amp = Envelope::default();
        self.amp.gate_on();
        self.filter_env = Envelope::default();
        self.filter_env.gate_on();
        self.own_env = own_env;
        self.released = false;
        self.one_shot = one_shot;
        self.kill = 1.0;
        self.kill_step = 0.0;
        self.ramp = if hard { 0.0 } else { 1.0 };
        self.ladder = [Ladder::default(); 2];
        self.svf = [Svf::default(); 2];
        self.control(ctl, kernels());
    }

    /// Moves to a new note without restarting (mono, legato).
    pub fn glide_to(&mut self, note: f32, ctl: &Control) {
        self.glide.set_time(ctl.glide_secs, ctl.control_rate);
        self.glide.target = note;
    }

    /// The key came up. A one-shot carries on unless `force` (the transport stopped).
    pub fn release(&mut self, force: bool) {
        if self.released || (self.one_shot && !force) {
            return;
        }
        self.released = true;
        self.amp.gate_off();
        self.filter_env.gate_off();
    }

    /// Fades out over a few milliseconds, for a voice stolen while it sounds.
    pub fn steal(&mut self, rate: f32) {
        self.owner = None;
        if !self.fading() {
            self.kill_step = 1.0 / (STEAL_FADE_SECS * rate);
        }
    }

    /// Once every control tick: pitch and filter.
    pub fn control(&mut self, ctl: &Control, k: &Kernels) {
        let Some(map) = &self.map else { return };
        let z = &map.zones[self.zone as usize];
        let note = self.glide.tick();
        let st = note as f64 + ctl.tune_st + ctl.lfo_pitch_st as f64 - z.root;
        // At its root, with nothing moving it, a sample plays at exactly its own rate.
        self.inc = if st == 0.0 { z.base_inc } else { z.base_inc * (st / 12.0).exp2() };
        self.bank = k.bank_index(self.inc) as u32;
        self.end_fade = (END_FADE * self.inc.max(1.0)).min((z.end - z.start) / 2.0);
        // The filter envelope runs here, its coefficients at the control rate.
        let fenv = self.filter_env.tick(&ctl.filter_env, 1.0);
        if ctl.filter != FilterKind::Off {
            let cut = ctl.cutoff_note + ctl.key_track * (note - 60.0) + ctl.env_st * fenv + ctl.vel_cutoff_st * self.velocity + ctl.lfo_filter_st;
            let g = cutoff_g(note_to_hz(cut), ctl.rate);
            match ctl.filter {
                FilterKind::Ladder24 => {
                    let res = ctl.resonance * 4.5;
                    self.ladder[0].set(g, res);
                    self.ladder[1].set(g, res);
                }
                // 0 % is Q 0.707, 100 % Q 20.
                _ => self.svf_coef = SvfCoef::new(g, 1.0 / (0.707 + 19.3 * ctl.resonance * ctl.resonance)),
            }
        }
        // A decay to a sustain of nothing has finished.
        if self.amp.stage == EnvStage::Decay && self.amp.level < END_LEVEL {
            self.clear();
        }
    }

    /// Adds `l.len()` samples of this voice into the mix.
    pub fn render(&mut self, l: &mut [f32], r: &mut [f32], ctl: &Control, k: &Kernels, scratch: &mut [f32; MAX_TAPS]) {
        let Some(map) = self.map.take() else {
            self.active = false;
            return;
        };
        let z = &map.zones[self.zone as usize];
        let bank: &Bank = k.bank(self.bank as usize);
        let plain = self.bank == 0;
        let stereo = z.sample.ch.len() > 1;
        let shape = self.own_env.unwrap_or(ctl.amp);
        let gain = self.vel_gain;
        let (gain_l, gain_r) = (z.gain_l * gain, z.gain_r * gain);
        let loop_len = z.looped.as_ref().map_or(0.0, |b| b.len as f64);
        let mut alive = true;
        for (ol, or) in l.iter_mut().zip(r.iter_mut()) {
            if !self.in_loop && self.pos >= self.enter_loop_at {
                self.in_loop = true;
                self.pos = (self.pos - z.loop_start).rem_euclid(loop_len);
            }
            if !self.in_loop && self.pos >= z.end {
                alive = false;
                break;
            }
            // Read a frame. Whole positions on the plain sinc are the samples themselves.
            let whole = self.pos.floor();
            let frac = (self.pos - whole) as f32;
            let at = GUARD + whole as usize;
            let (c0, c1): (&[f32], Option<&[f32]>) = if self.in_loop {
                let b = z.looped.as_ref().unwrap();
                (&b.ch[0], stereo.then(|| &*b.ch[1]))
            } else {
                (&z.sample.ch[0], stereo.then(|| &*z.sample.ch[1]))
            };
            let (mut a, mut b);
            if plain && frac == 0.0 {
                a = c0[at];
                b = c1.map_or(a, |c| c[at]);
            } else {
                let kernel = bank.blend(frac, scratch);
                a = bank.apply(kernel, c0, at);
                b = c1.map_or(a, |c| bank.apply(kernel, c, at));
            }
            // Move on.
            self.pos += self.inc;
            if self.in_loop && self.pos >= loop_len {
                self.pos -= loop_len;
                if self.pos >= loop_len {
                    self.pos = self.pos.rem_euclid(loop_len);
                }
            }
            match ctl.filter {
                FilterKind::Off => {}
                FilterKind::Ladder24 => {
                    a = self.ladder[0].tick(a);
                    b = if stereo { self.ladder[1].tick(b) } else { a };
                }
                kind => {
                    let c = self.svf_coef;
                    a = self.svf[0].tick(&c, kind, a);
                    b = if stereo { self.svf[1].tick(&c, kind, b) } else { a };
                }
            }
            let mut g = self.amp.tick(&shape, 1.0);
            if !self.in_loop {
                let left = z.end - self.pos;
                if left < self.end_fade {
                    g *= (left / self.end_fade).max(0.0) as f32;
                }
            }
            if self.ramp < 1.0 {
                g *= self.ramp;
                self.ramp = (self.ramp + 1.0 / START_RAMP).min(1.0);
            }
            if self.kill_step > 0.0 {
                g *= self.kill;
                self.kill -= self.kill_step;
                if self.kill <= 0.0 {
                    alive = false;
                    break;
                }
            }
            *ol += a * g * gain_l;
            *or += b * g * gain_r;
            if self.amp.stage == EnvStage::Idle {
                alive = false;
                break;
            }
        }
        if alive {
            self.map = Some(map);
        } else {
            self.active = false;
            self.owner = None;
            // Dropping the map's hold here is a decrement: the instance or the session has one.
            drop(map);
        }
    }

    #[cfg(test)]
    pub fn in_loop(&self) -> bool {
        self.in_loop
    }

    #[cfg(test)]
    pub fn ratio(&self) -> f64 {
        self.inc
    }
}


