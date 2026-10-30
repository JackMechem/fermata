//! A small built-in polyphonic synth so software-instrument tracks make sound without any
//! third-party instrument installed.

use vst3_host::MidiEvent;

const VOICES: usize = 16;

#[derive(Clone, Copy, Default)]
struct Voice {
    note: u8,
    active: bool,
    released: bool,
    /// Key let go while the sustain pedal was down: releases when the pedal comes up.
    pedal_held: bool,
    attacking: bool,
    phase_a: f32,
    phase_b: f32,
    freq: f32,
    vel: f32,
    env: f32,
    lp: f32,
    age: u64,
}

pub struct BasicSynth {
    sr: f32,
    voices: [Voice; VOICES],
    clock: u64,
    /// Pitch bend as a frequency ratio (±2 semitones).
    bend: f32,
    sustain: bool,
    /// Mod wheel: vibrato depth 0–1.
    modulation: f32,
    /// Expression (CC 11) as a gain.
    expression: f32,
    lfo: f32,
}

impl BasicSynth {
    pub fn new(sample_rate: f32) -> BasicSynth {
        BasicSynth { sr: sample_rate, voices: [Voice::default(); VOICES], clock: 0, bend: 1.0, sustain: false, modulation: 0.0, expression: 1.0, lfo: 0.0 }
    }

    pub fn all_notes_off(&mut self) {
        self.sustain = false;
        for v in &mut self.voices {
            v.released = true;
        }
    }

    /// Stops every voice at once, release tails included, and lets go of the pedal and bend:
    /// for offline renders, which must not start with the previous render's tail.
    pub fn silence(&mut self) {
        self.voices = [Voice::default(); VOICES];
        self.sustain = false;
        self.bend = 1.0;
    }

    fn handle(&mut self, data: [u8; 3]) {
        let status = data[0] & 0xF0;
        match status {
            0x90 if data[2] > 0 => {
                self.clock += 1;
                let slot = self
                    .voices
                    .iter()
                    .position(|v| !v.active)
                    .unwrap_or_else(|| (0..VOICES).min_by_key(|&i| self.voices[i].age).unwrap());
                self.voices[slot] = Voice {
                    note: data[1],
                    active: true,
                    released: false,
                    attacking: true,
                    freq: 440.0 * 2f32.powf((data[1] as f32 - 69.0) / 12.0),
                    vel: data[2] as f32 / 127.0,
                    age: self.clock,
                    ..Default::default()
                };
            }
            0x80 | 0x90 => {
                let sustain = self.sustain;
                for v in &mut self.voices {
                    if v.active && v.note == data[1] && !v.released {
                        if sustain {
                            v.pedal_held = true;
                        } else {
                            v.released = true;
                        }
                    }
                }
            }
            0xB0 if data[1] == 123 || data[1] == 120 => self.all_notes_off(),
            0xB0 if data[1] == 64 => {
                self.sustain = data[2] >= 64;
                if !self.sustain {
                    for v in self.voices.iter_mut().filter(|v| v.pedal_held) {
                        v.pedal_held = false;
                        v.released = true;
                    }
                }
            }
            0xB0 if data[1] == 1 => self.modulation = data[2] as f32 / 127.0,
            0xB0 if data[1] == 11 => self.expression = data[2] as f32 / 127.0,
            0xB0 if data[1] == 121 => {
                self.bend = 1.0;
                self.modulation = 0.0;
                self.expression = 1.0;
            }
            0xE0 => {
                let v = ((data[2] as i32 & 127) << 7 | (data[1] as i32 & 127)) - 8192;
                self.bend = 2f32.powf(v as f32 / 8192.0 * 2.0 / 12.0);
            }
            _ => {}
        }
    }

    pub fn process(&mut self, left: &mut [f32], right: &mut [f32], midi: &[MidiEvent]) {
        let n = left.len();
        let attack = 1.0 / (0.004 * self.sr);
        let release = 1.0 - 1.0 / (0.25 * self.sr);
        let decay = 1.0 - 1.0 / (1.8 * self.sr);
        let mut ev = midi.iter().peekable();
        let lfo_inc = 5.5 / self.sr;
        for i in 0..n {
            while let Some(e) = ev.peek() {
                if e.offset as usize <= i {
                    self.handle(e.data);
                    ev.next();
                } else {
                    break;
                }
            }
            self.lfo = (self.lfo + lfo_inc) % 1.0;
            // Up to half a semitone of vibrato on the mod wheel.
            let vibrato = if self.modulation > 0.0 { 1.0 + 0.029 * self.modulation * (self.lfo * std::f32::consts::TAU).sin() } else { 1.0 };
            let pitch = self.bend * vibrato;
            let mut out = 0.0f32;
            for v in self.voices.iter_mut().filter(|v| v.active) {
                if v.released {
                    v.env *= release;
                    if v.env < 1e-4 {
                        v.active = false;
                        continue;
                    }
                } else if v.attacking {
                    v.env += attack;
                    if v.env >= 1.0 {
                        v.env = 1.0;
                        v.attacking = false;
                    }
                } else {
                    // Decay towards a 0.35 sustain level.
                    v.env = v.env * decay + 0.35 * (1.0 - decay);
                }
                let inc = v.freq * pitch / self.sr;
                v.phase_a = (v.phase_a + inc) % 1.0;
                v.phase_b = (v.phase_b + inc * 1.005) % 1.0;
                let saw = (v.phase_a * 2.0 - 1.0) + (v.phase_b * 2.0 - 1.0);
                // One-pole low-pass whose cutoff follows velocity and envelope.
                let cutoff = (0.04 + 0.25 * v.vel * v.env).min(0.9);
                v.lp += cutoff * (saw - v.lp);
                out += v.lp * v.env * v.vel * 0.18;
            }
            left[i] = out * self.expression;
            right[i] = out * self.expression;
        }
        for e in ev {
            self.handle(e.data);
        }
    }
}
