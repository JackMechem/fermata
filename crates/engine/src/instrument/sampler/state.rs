//! A sampler's non-numeric settings: its zones. They live in the slot's `PluginSlot::state` as
//! JSON, so saving, undo, copy and paste and Freeze carry them like any other setting. Numeric
//! parameters stay in `PluginSlot::params`.
//!
//! Sample positions are frames of the file as it is on disk, whatever the project's rate.

use serde::{Deserialize, Serialize};

fn yes_127() -> u8 {
    127
}

/// One sample mapped to a range of keys and velocities.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Zone {
    pub id: u64,
    pub name: String,
    /// The file, relative to the project folder (or to an instrument's folder, in a saved one).
    pub file: String,
    #[serde(default)]
    pub key_lo: u8,
    #[serde(default = "yes_127")]
    pub key_hi: u8,
    #[serde(default)]
    pub vel_lo: u8,
    #[serde(default = "yes_127")]
    pub vel_hi: u8,
    /// The key at which the sample plays at its own pitch.
    pub root: u8,
    /// Cents.
    #[serde(default)]
    pub fine: f32,
    #[serde(default)]
    pub gain_db: f32,
    /// -1 left to 1 right.
    #[serde(default)]
    pub pan: f32,
    #[serde(default)]
    pub start: u64,
    /// One past the last frame; 0 is the end of the file.
    #[serde(default)]
    pub end: u64,
    #[serde(default)]
    pub loop_on: bool,
    #[serde(default)]
    pub loop_start: u64,
    /// One past the loop's last frame; 0 is the end of the zone.
    #[serde(default)]
    pub loop_end: u64,
    /// Frames of crossfade into the loop's end from the audio before its start.
    #[serde(default)]
    pub xfade: u64,
    /// Crossfade at equal power (for loops whose two sides don't line up), not equal gain.
    #[serde(default)]
    pub equal_power: bool,
    /// Plays to its end whatever the key does.
    #[serde(default)]
    pub one_shot: bool,
    #[serde(default)]
    pub reverse: bool,
    /// Zones of the same round-robin group on the same key and velocity take turns. 0 is none.
    #[serde(default)]
    pub rr_group: u16,
    /// This zone's own amp envelope (attack, decay and release in seconds, sustain 0..1), from an
    /// SFZ file. None follows the instrument's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<[f32; 4]>,
}

impl Zone {
    /// A zone over the whole keyboard and every velocity, playing the whole file.
    pub fn new(id: u64, name: String, file: String, root: u8) -> Zone {
        Zone {
            id,
            name,
            file,
            key_lo: 0,
            key_hi: 127,
            vel_lo: 0,
            vel_hi: 127,
            root,
            fine: 0.0,
            gain_db: 0.0,
            pan: 0.0,
            start: 0,
            end: 0,
            loop_on: false,
            loop_start: 0,
            loop_end: 0,
            xfade: 0,
            equal_power: false,
            one_shot: false,
            reverse: false,
            rr_group: 0,
            env: None,
        }
    }

    /// Keeps numbers in range, however they got here (an old file, a hand edit, the UI).
    pub fn tidy(&mut self) {
        let key = |v: &mut u8| *v = (*v).min(127);
        key(&mut self.key_lo);
        key(&mut self.key_hi);
        key(&mut self.vel_lo);
        key(&mut self.vel_hi);
        key(&mut self.root);
        if self.key_lo > self.key_hi {
            std::mem::swap(&mut self.key_lo, &mut self.key_hi);
        }
        if self.vel_lo > self.vel_hi {
            std::mem::swap(&mut self.vel_lo, &mut self.vel_hi);
        }
        let finite = |v: f32, d: f32| if v.is_finite() { v } else { d };
        self.fine = finite(self.fine, 0.0).clamp(-1200.0, 1200.0);
        self.gain_db = finite(self.gain_db, 0.0).clamp(-96.0, 24.0);
        self.pan = finite(self.pan, 0.0).clamp(-1.0, 1.0);
        self.rr_group = self.rr_group.min(super::map::MAX_RR_GROUPS as u16 - 1);
        if let Some(e) = &mut self.env {
            for (i, v) in e.iter_mut().enumerate() {
                *v = finite(*v, 0.0).clamp(0.0, if i == 2 { 1.0 } else { 120.0 });
            }
        }
    }
}

/// All a sampler's zones.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SamplerState {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub next_id: u64,
    #[serde(default)]
    pub zones: Vec<Zone>,
}

impl SamplerState {
    pub const VERSION: u32 = 1;

    /// Reads a slot's state. None or unreadable is an empty instrument.
    pub fn parse(state: Option<&str>) -> SamplerState {
        let mut s: SamplerState = state.and_then(|t| serde_json::from_str(t).map_err(|e| log::error!("a sampler's zones can't be read: {e}")).ok()).unwrap_or_default();
        s.zones.iter_mut().for_each(Zone::tidy);
        s.next_id = s.next_id.max(s.zones.iter().map(|z| z.id + 1).max().unwrap_or(1));
        s
    }

    pub fn to_json(&self) -> String {
        let doc = SamplerState { version: Self::VERSION, ..self.clone() };
        serde_json::to_string(&doc).unwrap_or_default()
    }

    pub fn alloc_id(&mut self) -> u64 {
        self.next_id = self.next_id.max(1);
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}
