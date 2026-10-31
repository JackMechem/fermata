//! Built-in instruments: the contract every one of them implements, and the registry that lists
//! them. The instrument counterpart of `crate::fx`, and it shares its [`ParamSpec`]s, presets and
//! panel plumbing.
//!
//! An instrument is plain DSP behind the [`Instrument`] trait. The engine runs it in a track's
//! instrument slot like a VST3 instrument; the UI shows it in a panel built from its spec.
//!
//! **Real-time rules (it runs on the audio thread):** the same as an effect's. Allocate in the
//! constructor; `process`, `set_param`, `reset` and `all_notes_off` must not allocate, lock or
//! block.
//!
//! **Determinism:** the same seed, parameters and MIDI after a `reset` give the same bits. Use no
//! global state, clock or thread-dependent value (`crate::synth_dsp::analog` has what character
//! needs). Parallel output being bit-identical to serial relies on this.
//!
//! **Parameters:** plain units, as for effects. Ids are stable forever: projects, automation lanes
//! and presets refer to them. Add new ids; never renumber.
//!
//! **To add a model:** write `instrument/<name>.rs` with `pub fn info() -> InstrumentInfo` and list
//! it in [`registry`]. Nothing else is needed: the menus, the panel (from `sections`), presets,
//! automation, MIDI learn, freezing and saving all work from the registry entry. See `tre/` for a
//! complete one.

use vst3_host::MidiEvent;

pub use crate::fx::{FactoryPreset, FxContext, ParamKind, ParamSpec, Unit, MAX_BLOCK, MAX_METERS};

/// Data an instrument plays from, built off the audio thread and handed over whole (see
/// [`Instrument::swap_data`]).
pub type InstrumentData = std::sync::Arc<dyn std::any::Any + Send + Sync>;

pub mod basic;
pub mod sampler;
pub mod tre;

/// A built-in instrument instance.
pub trait Instrument: Send {
    /// Sets a parameter (plain units, already clamped).
    fn set_param(&mut self, id: u32, value: f32);
    /// The current value of a parameter (what was set, not its smoothed state).
    fn param(&self, id: u32) -> f32;
    /// Plays one block, overwriting `l` and `r` (same length, at most [`MAX_BLOCK`]). `midi` is
    /// sorted by `offset`, the sample in this block each message lands on; apply them there:
    ///
    /// ```ignore
    /// let mut at = 0;
    /// for e in midi {
    ///     let to = (e.offset as usize).min(l.len());
    ///     self.render(&mut l[at..to], &mut r[at..to]);
    ///     at = to;
    ///     self.handle(Midi::parse(e.data));
    /// }
    /// self.render(&mut l[at..], &mut r[at..]);
    /// ```
    ///
    /// `ctx` has the sample rate, tempo and song position, for tempo-synced LFOs.
    fn process(&mut self, l: &mut [f32], r: &mut [f32], midi: &[MidiEvent], ctx: &FxContext);
    /// All notes end as if their keys came up, and the pedal lifts. Release tails ring on. The
    /// engine calls this when the transport stops or jumps.
    fn all_notes_off(&mut self);
    /// Stops dead and clears everything played so far: voices, tails, filters, MIDI controllers
    /// and random generators, as a new instance with these parameters would be, smoothed values
    /// landing on their targets. Offline renders start and end with this so they repeat exactly,
    /// and a track going Off gets it.
    fn reset(&mut self);
    /// The longest it keeps sounding after every note ended, at the current settings, so
    /// bounces and freezes keep the tail.
    fn tail_secs(&self) -> f32 {
        0.0
    }
    /// Writes the values listed in [`InstrumentInfo::meters`], in that order, for the panel.
    fn meters(&self, out: &mut [f32; MAX_METERS]) {
        let _ = out;
    }
    /// Takes over data that isn't numeric (a sampler's zones and audio), and gives back what it
    /// replaced for the engine to return to the session. Runs on the audio thread, so only swaps
    /// pointers. The instrument may go on reading the old data through clones of its `Arc` (notes
    /// still ringing): dropping those is safe here, because the session keeps its own hold on what
    /// comes back until it is the last holder, and frees it then, on its own thread.
    ///
    /// The settings behind the data live in `PluginSlot::state`, which saving, undo, copying and
    /// Freeze carry. The session builds the data from them before sending it, so there's nothing
    /// for an offline render to wait for.
    fn swap_data(&mut self, data: InstrumentData) -> Option<InstrumentData> {
        Some(data)
    }
}

/// A MIDI message an instrument cares about, in its own terms.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Midi {
    /// `velocity` 0..1, never 0 (a note-on with velocity 0 is a [`Midi::NoteOff`]).
    NoteOn { note: u8, velocity: f32 },
    NoteOff { note: u8 },
    /// -1..1, 0 centred.
    PitchBend(f32),
    /// CC 1, 0..1.
    ModWheel(f32),
    /// CC 64.
    Sustain(bool),
    /// CC 11, 0..1.
    Expression(f32),
    /// Channel pressure, 0..1.
    Aftertouch(f32),
    /// CC 120 or 123.
    AllNotesOff,
    /// CC 121: bend, wheel, expression and aftertouch go back to rest.
    ResetControllers,
    /// Anything else.
    Other,
}

impl Midi {
    /// Decodes a message, on any channel.
    #[inline]
    pub fn parse(data: [u8; 3]) -> Midi {
        let unit = |v: u8| (v & 127) as f32 / 127.0;
        match data[0] & 0xF0 {
            0x90 if data[2] > 0 => Midi::NoteOn { note: data[1] & 127, velocity: unit(data[2]) },
            0x80 | 0x90 => Midi::NoteOff { note: data[1] & 127 },
            0xE0 => {
                let v = ((data[2] as i32 & 127) << 7 | (data[1] as i32 & 127)) - 8192;
                Midi::PitchBend(if v < 0 { v as f32 / 8192.0 } else { v as f32 / 8191.0 })
            }
            0xD0 => Midi::Aftertouch(unit(data[1])),
            0xB0 => match data[1] {
                1 => Midi::ModWheel(unit(data[2])),
                11 => Midi::Expression(unit(data[2])),
                64 => Midi::Sustain(data[2] >= 64),
                120 | 123 => Midi::AllNotesOff,
                121 => Midi::ResetControllers,
                _ => Midi::Other,
            },
            _ => Midi::Other,
        }
    }
}

/// A titled group of parameters in an instrument's panel.
#[derive(Debug, Clone, Copy)]
pub struct Section {
    pub title: &'static str,
    /// The parameters it shows, by id, in order.
    pub params: &'static [u32],
}

/// A built-in instrument's description in the registry.
pub struct InstrumentInfo {
    /// The plug-in id used in slots and saved projects, for example "builtin:tre". Forever.
    pub id: &'static str,
    /// The name in menus and the panel. No trademarks: see docs/ARCHITECTURE.md.
    pub name: &'static str,
    /// The menu group: "Fermata" (house instruments), "Vintage Synths", "Samplers" and so on.
    pub group: &'static str,
    /// A line for the Library's tooltip.
    pub about: &'static str,
    pub params: &'static [ParamSpec],
    /// The panel's layout. Every parameter should appear in one (a test checks).
    pub sections: &'static [Section],
    /// Names of the meter values it reports, in order.
    pub meters: &'static [&'static str],
    /// Its factory presets. Parameters a preset doesn't list go to their defaults.
    pub presets: &'static [FactoryPreset],
    /// Makes an instance at a sample rate, with every parameter at its default. `seed` is the
    /// slot's id: analog-style differences come from it, so they stay with the instance across
    /// saving and reopening, and differ between instances.
    pub new: fn(sample_rate: f32, seed: u64) -> Box<dyn Instrument>,
}

impl InstrumentInfo {
    pub fn param(&self, id: u32) -> Option<&ParamSpec> {
        self.params.iter().find(|p| p.id == id)
    }
}

/// Every built-in instrument. To add one: write its module and list it here.
pub fn registry() -> Vec<InstrumentInfo> {
    vec![basic::info(), tre::info(), sampler::info()]
}

/// The registry entry for a plug-in id.
pub fn find(id: &str) -> Option<InstrumentInfo> {
    registry().into_iter().find(|e| e.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn registry_entries_are_well_formed() {
        let reg = registry();
        let mut ids = HashSet::new();
        for e in &reg {
            assert!(ids.insert(e.id), "{} is listed twice", e.id);
            assert!(e.id.starts_with("builtin:"), "{}", e.id);
            assert!(crate::fx::find(e.id).is_none(), "{} is an effect's id too", e.id);
            let mut seen = HashSet::new();
            for p in e.params {
                assert!(seen.insert(p.id), "{}: id {} twice", e.id, p.id);
                assert!(p.default >= p.min && p.default <= p.max, "{}: {} default out of range", e.id, p.name);
                if p.log {
                    assert!(p.min > 0.0, "{}: {} is logarithmic from {}", e.id, p.name, p.min);
                }
            }
            let mut in_section = HashSet::new();
            for s in e.sections {
                for id in s.params {
                    assert!(seen.contains(id), "{}: section {} lists unknown {id}", e.id, s.title);
                    assert!(in_section.insert(*id), "{}: {id} in two sections", e.id);
                }
            }
            assert_eq!(in_section.len(), e.params.len(), "{}: a parameter is in no section", e.id);
            for preset in e.presets {
                for (id, v) in preset.values {
                    let spec = e.param(*id).unwrap_or_else(|| panic!("{}: {} sets unknown {id}", e.id, preset.name));
                    assert!(*v >= spec.min && *v <= spec.max, "{}: {} {} out of range", e.id, preset.name, spec.name);
                }
            }
            // Instances start and run without the rest of the engine.
            let mut i = (e.new)(48000.0, 1);
            let (mut l, mut r) = (vec![0.0; 64], vec![0.0; 64]);
            let ctx = FxContext { sample_rate: 48000.0, tempo: 120.0, playing: true, position: 0.0 };
            i.process(&mut l, &mut r, &[MidiEvent { offset: 3, data: [0x90, 60, 100] }], &ctx);
            for p in e.params {
                assert_eq!(i.param(p.id), p.default, "{}: {}", e.id, p.name);
            }
        }
    }

    #[test]
    fn midi_decodes() {
        assert_eq!(Midi::parse([0x93, 60, 127]), Midi::NoteOn { note: 60, velocity: 1.0 });
        assert_eq!(Midi::parse([0x90, 60, 0]), Midi::NoteOff { note: 60 });
        assert_eq!(Midi::parse([0x80, 60, 40]), Midi::NoteOff { note: 60 });
        assert_eq!(Midi::parse([0xE0, 0, 64]), Midi::PitchBend(0.0));
        assert_eq!(Midi::parse([0xE0, 127, 127]), Midi::PitchBend(1.0));
        assert_eq!(Midi::parse([0xE0, 0, 0]), Midi::PitchBend(-1.0));
        assert_eq!(Midi::parse([0xB0, 1, 127]), Midi::ModWheel(1.0));
        assert_eq!(Midi::parse([0xB0, 11, 0]), Midi::Expression(0.0));
        assert_eq!(Midi::parse([0xB0, 64, 64]), Midi::Sustain(true));
        assert_eq!(Midi::parse([0xB0, 64, 63]), Midi::Sustain(false));
        assert_eq!(Midi::parse([0xD0, 127, 0]), Midi::Aftertouch(1.0));
        assert_eq!(Midi::parse([0xB0, 123, 0]), Midi::AllNotesOff);
        assert_eq!(Midi::parse([0xB0, 121, 0]), Midi::ResetControllers);
        assert_eq!(Midi::parse([0xB0, 74, 3]), Midi::Other);
    }
}
