//! Fermata Synth (`crate::synth::BasicSynth`, untouched) in the instrument registry: no parameters,
//! no panel. Its id and sound are what every existing project holds, so it stays as it always was.
//! It renders what it did before this contract existed: `silence` still keeps the mod wheel,
//! expression and its LFO's phase, for instance, unlike the contract's `reset`, because changing
//! that would change old projects.

use vst3_host::MidiEvent;

use super::{FxContext, Instrument, InstrumentInfo};
use crate::synth::BasicSynth;

struct Basic(BasicSynth);

impl Instrument for Basic {
    fn set_param(&mut self, _: u32, _: f32) {}

    fn param(&self, _: u32) -> f32 {
        0.0
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], midi: &[MidiEvent], _: &FxContext) {
        self.0.process(l, r, midi);
    }

    fn all_notes_off(&mut self) {
        self.0.all_notes_off();
    }

    fn reset(&mut self) {
        self.0.silence();
    }
}

pub fn info() -> InstrumentInfo {
    InstrumentInfo {
        id: crate::session::BUILTIN_SYNTH,
        name: "Fermata Synth",
        group: "Fermata",
        about: "A small polyphonic synth with no settings",
        params: &[],
        sections: &[],
        meters: &[],
        presets: &[],
        new: |sample_rate, _| Box::new(Basic(BasicSynth::new(sample_rate))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The registry's Fermata Synth is the old one, sample for sample, through everything the
    /// engine does to it: notes past the 16 voices, bend, wheel, pedal, expression, stops and
    /// renders starting.
    #[test]
    fn plays_exactly_what_the_old_synth_played() {
        let mut new = (info().new)(48000.0, 12345);
        let mut old = BasicSynth::new(48000.0);
        let ctx = FxContext { sample_rate: 48000.0, tempo: 120.0, playing: true, position: 0.0 };
        let msg = |k: u32| -> [u8; 3] {
            match k % 11 {
                0..=4 => [0x90, 30 + (k * 7 % 60) as u8, 20 + (k * 13 % 107) as u8],
                5 | 6 => [0x80, 30 + ((k - 15) * 7 % 60) as u8, 0],
                7 => [0xE0, (k % 127) as u8, 30 + (k % 60) as u8],
                8 => [0xB0, 1, (k % 128) as u8],
                9 => [0xB0, 11, 60 + (k % 68) as u8],
                _ => [0xB0, 64, if k.is_multiple_of(3) { 127 } else { 0 }],
            }
        };
        for block in 0..600u32 {
            let midi: Vec<MidiEvent> = (0..4).map(|i| MidiEvent { offset: i * 100, data: msg(block * 4 + i + 16) }).collect();
            let (mut a, mut b) = (vec![0.0f32; 512], vec![0.0f32; 512]);
            let (mut c, mut d) = (vec![0.0f32; 512], vec![0.0f32; 512]);
            new.process(&mut a, &mut b, &midi, &ctx);
            old.process(&mut c, &mut d, &midi);
            assert!(a == c && b == d, "block {block}");
            if block % 97 == 96 {
                new.all_notes_off();
                old.all_notes_off();
            }
            if block % 211 == 210 {
                new.reset();
                old.silence();
            }
        }
    }
}
