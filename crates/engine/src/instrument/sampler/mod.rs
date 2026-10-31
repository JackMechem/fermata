//! Fermata Sampler: samples mapped across keys and velocities, in memory, with loops, round robin,
//! an amp envelope, a filter with its envelope, an LFO, glide and polyphony.
//!
//! - [`state`]: the zones as saved in the slot's `PluginSlot::state`.
//! - [`sample`]: the decoded files, held off the audio thread, with a memory limit.
//! - [`map`]: the zones made ready to play, handed to the instance whole.
//! - [`kernel`]: the interpolator.
//! - [`voice`]: one zone sounding.
//! - [`mapping`]: telling the note from a file's name, and placing files on keys.
//! - [`sfz`]: reading SFZ files.
//!
//! **On the audio thread** nothing allocates, locks or blocks. A new map arrives through
//! [`Instrument::swap_data`]; notes already sounding keep the map they started with, by a clone of
//! its `Arc`, so a change never cuts one off. Letting go of a clone is only a decrement, because
//! the instance holds the current map and the session holds the old ones until nothing else does.
//!
//! **Stealing.** A sample's phase can't carry over, so a stolen voice fades over 4 ms while the new
//! note starts on another. The allocator hands out notes up to the polyphony; the voices behind it
//! are many more, for layers and for those fades.

use std::sync::Arc;

use vst3_host::MidiEvent;

use super::{Instrument, InstrumentData, InstrumentInfo, Midi, FxContext, MAX_METERS};
use crate::synth_dsp::alloc::{Priority, VoiceAction, VoiceAllocator, VoiceMode};
use crate::synth_dsp::env::EnvShape;
use crate::synth_dsp::lfo::{Lfo, LfoRate, LfoWave};
use crate::synth_dsp::smooth::Smoothed;

pub mod kernel;
pub mod map;
pub mod mapping;
pub mod params;
pub mod sample;
pub mod sfz;
pub mod state;
pub mod voice;

use kernel::{kernels, Kernels, MAX_TAPS};
use map::{SamplerMap, MAX_RR_GROUPS};
use params::param::*;
use params::{LFO_SYNC_PER_BEAT, PARAMS, POLY_COUNTS, PRESETS, SECTIONS};
use voice::{hz_to_note, Control, FilterKind, Voice};

/// The plug-in id, forever.
pub const ID: &str = "builtin:sampler";
/// Notes at once, at most (the Polyphony parameter's top).
pub const MAX_NOTES: usize = 64;
/// Zones one note can sound together.
pub const MAX_LAYERS: usize = 8;
/// Voices behind the notes: two layers on every note, and spares for fades.
const VOICES: usize = MAX_NOTES * 2 + SPARE;
/// Voices kept free for a new note while a stolen one fades.
const SPARE: usize = 16;
/// Samples between control ticks: pitch, filter coefficients, LFO.
pub const CONTROL: usize = 32;
/// How much of a release's stated time rings on: it reaches the end level this many times later
/// than the 95 % point the time names.
const RELEASE_RING: f32 = 3.84;

/// The voices, and all they need when the allocator tells them something.
struct Bank {
    voices: Vec<Voice>,
    map: Arc<SamplerMap>,
    /// How many notes each round-robin group has played.
    rr: [u32; MAX_RR_GROUPS],
    control: Control,
    age: u64,
    /// The note before this one, for glide.
    last_note: Option<u8>,
    transpose: f64,
    last_played: (f32, f32),
}

impl Bank {
    fn act(&mut self, a: VoiceAction) {
        match a {
            VoiceAction::Start { voice, note, velocity, stolen, .. } => {
                if stolen {
                    let rate = self.control.rate;
                    for v in self.voices.iter_mut().filter(|v| v.active && v.owner == Some(voice as u8)) {
                        v.steal(rate);
                    }
                }
                self.start_note(voice as u8, note, velocity);
            }
            VoiceAction::Glide { voice, note } => {
                let ctl = self.control;
                for v in self.voices.iter_mut().filter(|v| v.active && v.owner == Some(voice as u8)) {
                    v.glide_to(note as f32, &ctl);
                }
                self.last_note = Some(note);
            }
            VoiceAction::Release { voice } => {
                for v in self.voices.iter_mut().filter(|v| v.active && v.owner == Some(voice as u8)) {
                    v.release(false);
                }
            }
        }
    }

    /// Starts a voice for each zone that answers this key and velocity.
    fn start_note(&mut self, owner: u8, note: u8, velocity: f32) {
        let vel = (velocity * 127.0).round() as u8;
        let map = self.map.clone();
        // Who plays: every zone in no group, and from each group the one whose turn it is.
        let mut groups = [(0u16, 0u32, 0u32); 16];
        let mut used = 0;
        let hits = |z: &map::PlayZone| vel >= z.vel_lo && vel <= z.vel_hi;
        for &i in map.zones_for_key(note) {
            let z = &map.zones[i as usize];
            if z.rr_group == 0 || !hits(z) {
                continue;
            }
            if let Some(g) = groups[..used].iter_mut().find(|g| g.0 == z.rr_group) {
                g.1 += 1;
            } else if used < groups.len() {
                groups[used] = (z.rr_group, 1, 0);
                used += 1;
            }
        }
        let mut layers = 0;
        let from = (self.control.glide_secs > 0.0).then_some(self.last_note).flatten().map(|n| n as f32);
        for &i in map.zones_for_key(note) {
            let z = &map.zones[i as usize];
            if !hits(z) {
                continue;
            }
            if z.rr_group != 0 {
                let Some(g) = groups[..used].iter_mut().find(|g| g.0 == z.rr_group) else { continue };
                let turn = self.rr[z.rr_group as usize] % g.1;
                let mine = g.2 == turn;
                g.2 += 1;
                if !mine {
                    continue;
                }
            }
            if layers == MAX_LAYERS {
                break;
            }
            layers += 1;
            let v = self.free_voice();
            self.age += 1;
            let (age, ctl) = (self.age, self.control);
            self.voices[v].start(&map, i, owner, age, note as f32, from, velocity, &ctl);
            self.last_played = (note as f32, z.id as f32);
        }
        for g in &groups[..used] {
            let c = &mut self.rr[g.0 as usize];
            *c = c.wrapping_add(1);
        }
        self.last_note = Some(note);
    }

    /// A free voice, keeping some spare by starting the oldest on their way out.
    fn free_voice(&mut self) -> usize {
        let free = self.voices.iter().filter(|v| !v.active).count();
        if free <= SPARE {
            let rate = self.control.rate;
            if let Some(v) = self.voices.iter_mut().filter(|v| v.active && !v.fading()).min_by_key(|v| v.age) {
                v.steal(rate);
            }
        }
        if let Some(i) = self.voices.iter().position(|v| !v.active) {
            return i;
        }
        // Out of voices altogether: cut the one nearest silence.
        let mut best = 0;
        for (i, v) in self.voices.iter().enumerate() {
            if v.kill_level() < self.voices[best].kill_level() {
                best = i;
            }
        }
        self.voices[best].clear();
        best
    }
}

pub struct Sampler {
    rate: f32,
    values: Vec<f32>,
    dirty: bool,
    bank: Bank,
    alloc: VoiceAllocator<MAX_NOTES>,
    lfo: Lfo,
    volume: Smoothed,
    /// Samples until the next control tick.
    until_control: usize,
    bend: f32,
    wheel: f32,
    expression: f32,
    scratch: Box<[f32; MAX_TAPS]>,
    kernels: &'static Kernels,
}

impl Sampler {
    pub fn new(sample_rate: f32, seed: u64) -> Sampler {
        let values = params::PARAMS.iter().map(|p| p.default).collect();
        let mut s = Sampler {
            rate: sample_rate,
            values,
            dirty: true,
            bank: Bank { voices: (0..VOICES).map(|_| Voice::default()).collect(), map: Arc::new(SamplerMap::default()), rr: [0; MAX_RR_GROUPS], control: Control::default(), age: 0, last_note: None, transpose: 0.0, last_played: (-1.0, -1.0) },
            alloc: VoiceAllocator::new(),
            lfo: Lfo::new(LfoWave::Sine, LfoRate::Hz(5.0), seed),
            volume: Smoothed::new(1.0, 8.0, sample_rate),
            until_control: 0,
            bend: 0.0,
            wheel: 0.0,
            expression: 1.0,
            scratch: Box::new([0.0; MAX_TAPS]),
            kernels: kernels(),
        };
        s.apply();
        s.volume.snap();
        s
    }

    #[inline]
    fn v(&self, id: u32) -> f32 {
        self.values[id as usize]
    }

    /// Works out what the parameters mean, once per block at most.
    fn apply(&mut self) {
        self.dirty = false;
        let rate = self.rate;
        let secs = |id: u32| self.v(id) / 1000.0;
        let c = &mut Control::default();
        c.rate = rate;
        c.control_rate = rate / CONTROL as f32;
        // At the slider's bottom an envelope has no attack at all, which a drum wants.
        let attack = |ms: f32| if ms <= 0.1 { 0.0 } else { ms / 1000.0 };
        c.amp = EnvShape::new(attack(self.v(A_ATTACK)), secs(A_DECAY), self.v(A_SUSTAIN), secs(A_RELEASE), rate);
        c.filter_env = EnvShape::new(attack(self.v(F_ATTACK)), secs(F_DECAY), self.v(F_SUSTAIN), secs(F_RELEASE), c.control_rate);
        c.vel_amp = self.v(VEL_AMP);
        c.filter = match self.v(F_TYPE) as usize {
            1 => FilterKind::Low12,
            2 => FilterKind::Band12,
            3 => FilterKind::High12,
            4 => FilterKind::Ladder24,
            _ => FilterKind::Off,
        };
        c.cutoff_note = hz_to_note(self.v(CUTOFF));
        c.resonance = self.v(RESONANCE);
        c.key_track = self.v(KEY_TRACK);
        c.env_st = self.v(F_ENV);
        c.vel_cutoff_st = self.v(VEL_CUTOFF);
        c.glide_secs = secs(GLIDE);
        let control = *c;
        self.bank.control = control;
        self.bank.transpose = self.v(TRANSPOSE).round() as f64 + self.v(TUNE) as f64 / 100.0;
        self.update_mode();
        let wave = match self.v(LFO_WAVE) as usize {
            1 => LfoWave::Triangle,
            2 => LfoWave::Saw,
            3 => LfoWave::Square,
            4 => LfoWave::SampleHold,
            _ => LfoWave::Sine,
        };
        self.lfo.set_wave(wave);
        let sync = LFO_SYNC_PER_BEAT[(self.v(LFO_SYNC) as usize).min(LFO_SYNC_PER_BEAT.len() - 1)];
        self.lfo.set_rate(if sync > 0.0 { LfoRate::PerBeat(sync) } else { LfoRate::Hz(self.v(LFO_RATE)) });
        self.update_volume();
        self.update_control_values();
    }

    fn update_volume(&mut self) {
        self.volume.target = 10f32.powf(self.v(VOLUME) / 20.0) * self.expression;
    }

    fn update_mode(&mut self) {
        let mode = match self.v(MODE) as usize {
            1 => VoiceMode::Mono { priority: Priority::Last, multi_trigger: true },
            2 => VoiceMode::Mono { priority: Priority::Last, multi_trigger: false },
            _ => VoiceMode::Poly,
        };
        let poly = POLY_COUNTS[(self.v(POLYPHONY) as usize).min(POLY_COUNTS.len() - 1)];
        let bank = &mut self.bank;
        self.alloc.set_mode(mode, 1, poly, |a| bank.act(a));
    }

    /// What bend and transposition add, which a new note needs at once.
    fn update_control_values(&mut self) {
        self.bank.control.tune_st = self.bank.transpose + (self.bend * self.v(BEND)) as f64;
    }

    /// A control tick: the LFO and everything the voices work out from it.
    fn control_tick(&mut self) {
        self.until_control = CONTROL;
        let lfo = self.lfo.tick();
        self.update_control_values();
        let vibrato = self.v(LFO_PITCH) + self.wheel * self.v(WHEEL_PITCH);
        let c = &mut self.bank.control;
        c.lfo_pitch_st = lfo * vibrato / 100.0;
        c.lfo_filter_st = lfo * self.values[LFO_FILTER as usize];
        let ctl = self.bank.control;
        let mut owned = [false; MAX_NOTES];
        for v in self.bank.voices.iter_mut().filter(|v| v.active) {
            v.control(&ctl, self.kernels);
            if let Some(o) = v.owner.filter(|_| v.active) {
                owned[o as usize] = true;
            }
        }
        // Notes whose voices have all ended are free. Only ones released count: see the allocator.
        for (i, o) in owned.iter().enumerate() {
            if !o {
                self.alloc.voice_finished(i);
            }
        }
    }

    fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len();
        let mut i = 0;
        while i < n {
            if self.until_control == 0 {
                self.control_tick();
            }
            let m = self.until_control.min(n - i);
            let (cl, cr) = (&mut l[i..i + m], &mut r[i..i + m]);
            let ctl = self.bank.control;
            for v in self.bank.voices.iter_mut().filter(|v| v.active) {
                v.render(cl, cr, &ctl, self.kernels, &mut self.scratch);
            }
            for (a, b) in cl.iter_mut().zip(cr.iter_mut()) {
                let g = self.volume.tick();
                *a *= g;
                *b *= g;
            }
            self.until_control -= m;
            i += m;
        }
    }

    fn handle(&mut self, m: Midi) {
        let bank = &mut self.bank;
        match m {
            Midi::NoteOn { note, velocity } => self.alloc.note_on(note, velocity, |a| bank.act(a)),
            Midi::NoteOff { note } => self.alloc.note_off(note, |a| bank.act(a)),
            Midi::PitchBend(v) => {
                self.bend = v;
                self.update_control_values();
            }
            Midi::ModWheel(v) => self.wheel = v,
            Midi::Sustain(down) => self.alloc.set_pedal(down, |a| bank.act(a)),
            Midi::Expression(v) => {
                self.expression = v;
                self.update_volume();
            }
            Midi::AllNotesOff => self.all_notes_off(),
            Midi::ResetControllers => {
                self.bend = 0.0;
                self.wheel = 0.0;
                self.expression = 1.0;
                self.update_volume();
                self.update_control_values();
            }
            Midi::Aftertouch(_) | Midi::Other => {}
        }
    }
}

impl Instrument for Sampler {
    fn set_param(&mut self, id: u32, value: f32) {
        if let Some(v) = self.values.get_mut(id as usize) {
            if *v != value {
                *v = value;
                self.dirty = true;
            }
        }
    }

    fn param(&self, id: u32) -> f32 {
        self.values.get(id as usize).copied().unwrap_or(0.0)
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], midi: &[MidiEvent], ctx: &FxContext) {
        if self.dirty {
            self.apply();
        }
        let n = l.len().min(r.len());
        l[..n].fill(0.0);
        r[..n].fill(0.0);
        self.lfo.begin_block(ctx, CONTROL as f64);
        let mut at = 0;
        for e in midi {
            let to = (e.offset as usize).clamp(at, n);
            self.render(&mut l[at..to], &mut r[at..to]);
            at = to;
            self.handle(Midi::parse(e.data));
        }
        self.render(&mut l[at..n], &mut r[at..n]);
    }

    fn all_notes_off(&mut self) {
        let bank = &mut self.bank;
        self.alloc.release_all(|a| bank.act(a));
        // Everything ends now, one-shots too: a stopped song shouldn't keep playing a long one.
        for v in self.bank.voices.iter_mut().filter(|v| v.active) {
            v.release(true);
        }
    }

    fn reset(&mut self) {
        if self.dirty {
            self.apply();
        }
        for v in self.bank.voices.iter_mut() {
            v.clear();
        }
        self.alloc.reset();
        self.bank.rr = [0; MAX_RR_GROUPS];
        self.bank.age = 0;
        self.bank.last_note = None;
        self.bank.last_played = (-1.0, -1.0);
        self.lfo.reset();
        self.until_control = 0;
        self.bend = 0.0;
        self.wheel = 0.0;
        self.expression = 1.0;
        self.update_volume();
        self.update_control_values();
        self.volume.snap();
    }

    fn tail_secs(&self) -> f32 {
        let release = (self.v(A_RELEASE) / 1000.0).max(self.bank.map.max_release);
        release * RELEASE_RING + self.bank.map.one_shot_tail
    }

    fn meters(&self, out: &mut [f32; MAX_METERS]) {
        out[0] = self.bank.voices.iter().filter(|v| v.active).count() as f32;
        out[1] = self.bank.last_played.0;
        out[2] = self.bank.last_played.1;
    }

    fn swap_data(&mut self, data: InstrumentData) -> Option<InstrumentData> {
        match data.downcast::<SamplerMap>() {
            Ok(map) => {
                let old = std::mem::replace(&mut self.bank.map, map);
                Some(old as InstrumentData)
            }
            Err(other) => Some(other),
        }
    }
}

pub fn info() -> InstrumentInfo {
    InstrumentInfo {
        id: ID,
        name: "Fermata Sampler",
        group: "Samplers",
        about: "Your own samples across keys and velocities, with loops, round robin and SFZ loading",
        params: PARAMS,
        sections: SECTIONS,
        meters: &["Voices", "Last Note", "Last Zone"],
        presets: PRESETS,
        new: |sample_rate, seed| Box::new(Sampler::new(sample_rate, seed)),
    }
}

#[cfg(test)]
mod tests;
