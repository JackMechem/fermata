//! A sampler's zones made ready to play: built off the audio thread from the state and the
//! sample pool, handed to the instance whole through `Instrument::swap_data`, and immutable from
//! then on. Voices hold it while they sound, so replacing it never touches a note in progress,
//! and only the session ever frees one (see `Session::sampler_idle`).

use std::path::Path;
use std::sync::Arc;

use super::kernel::GUARD;
use super::sample::{LoadError, LoopKey, SampleBuf, SamplePool};
use super::state::{SamplerState, Zone};
use crate::synth_dsp::env::EnvShape;

/// Round-robin groups available, numbered 1 up; 0 is no group.
pub const MAX_RR_GROUPS: usize = 256;
/// How long a one-shot's tail is counted for, however low it's played.
const MAX_ONE_SHOT_TAIL: f32 = 60.0;

/// One loop's audio, one period long with the crossfade applied, and its own continuation on each
/// side, so a kernel anywhere in it reads the loop going round. Each channel has `len + 2 * GUARD`
/// values; frame `k` of the loop is at `GUARD + k`.
pub struct LoopBuf {
    pub len: usize,
    pub ch: Vec<Box<[f32]>>,
    /// The crossfade used, in frames: what was asked for, less where the audio before the loop's
    /// start ran out.
    pub xfade: usize,
}

impl LoopBuf {
    /// Bakes `[start, end)` of `sample`. The last `xfade` frames fade out as the audio before
    /// `start` fades in, which is what the loop's start continues from, so the seam is inside
    /// real audio. A zone whose loop starts too early for its crossfade gets a shorter one.
    pub fn bake(sample: &SampleBuf, start: usize, end: usize, xfade: usize, equal_power: bool) -> LoopBuf {
        let len = end - start;
        let xfade = xfade.min(start).min(len);
        let ch = (0..sample.ch.len())
            .map(|c| {
                let src = sample.frames_of(c);
                let mut body: Vec<f32> = src[start..end].to_vec();
                for k in 0..xfade {
                    // 0 at the first faded frame, approaching 1 at the loop's end.
                    let t = (k + 1) as f32 / (xfade + 1) as f32;
                    let at = len - xfade + k;
                    let before = src[start + k - xfade];
                    let (gain_out, gain_in) = if equal_power { ((1.0 - t).sqrt(), t.sqrt()) } else { (1.0 - t, t) };
                    body[at] = body[at] * gain_out + before * gain_in;
                }
                // Round and round on either side, however short the loop is.
                let mut v = vec![0.0f32; len + 2 * GUARD];
                for (i, o) in v.iter_mut().enumerate() {
                    *o = body[(i + len * (GUARD / len + 1) - GUARD) % len];
                }
                v.into_boxed_slice()
            })
            .collect();
        LoopBuf { len, ch, xfade }
    }

    pub fn bytes(&self) -> usize {
        self.ch.iter().map(|c| c.len() * 4).sum()
    }
}

/// A zone ready to play. Positions are frames of `sample` (which is already backwards if the zone
/// plays in reverse).
pub struct PlayZone {
    pub id: u64,
    pub key_lo: u8,
    pub key_hi: u8,
    pub vel_lo: u8,
    pub vel_hi: u8,
    /// The key that plays at the sample's own pitch, with the zone's fine tune taken in.
    pub root: f64,
    /// Source frames per output sample at that pitch: the file's rate over the project's.
    pub base_inc: f64,
    pub gain_l: f32,
    pub gain_r: f32,
    pub sample: Arc<SampleBuf>,
    pub looped: Option<Arc<LoopBuf>>,
    pub start: f64,
    pub end: f64,
    pub loop_start: f64,
    pub one_shot: bool,
    pub rr_group: u16,
    pub env: Option<EnvShape>,
    /// Starts away from silence: the voice fades it in over a moment rather than click.
    pub hard_start: bool,
}

/// The zones of one sampler instance.
#[derive(Default)]
pub struct SamplerMap {
    pub zones: Vec<PlayZone>,
    /// For each key, the part of `by_key` listing the zones on it, in zone order.
    pub key_index: Vec<(u32, u32)>,
    pub by_key: Vec<u32>,
    /// The longest a one-shot can ring, in seconds.
    pub one_shot_tail: f32,
    /// Audio this map keeps alive beyond what the pool holds: the baked loops.
    pub loop_bytes: usize,
    /// The longest release any zone's own envelope asks for, in seconds.
    pub max_release: f32,
}

/// A zone that can't play, and why.
#[derive(Debug, Clone, PartialEq)]
pub struct ZoneIssue {
    pub zone: u64,
    pub file: String,
    pub error: LoadError,
}

impl SamplerMap {
    /// Builds a map from the state. Files resolve against `base`. A zone that can't load is left
    /// out and reported; the others play. Decodes what the pool doesn't hold, so this can take a
    /// while, and must not run on the audio thread.
    pub fn build(state: &SamplerState, base: &Path, out_rate: f32, pool: &SamplePool) -> (SamplerMap, Vec<ZoneIssue>) {
        let mut map = SamplerMap::default();
        let mut issues = Vec::new();
        for z in &state.zones {
            match Self::play_zone(z, base, out_rate as f64, pool) {
                Ok(p) => {
                    map.max_release = map.max_release.max(z.env.map_or(0.0, |e| e[3]));
                    map.loop_bytes += p.looped.as_ref().map_or(0, |l| l.bytes());
                    if p.one_shot {
                        let lowest = 2f64.powf((z.key_lo as f64 - p.root) / 12.0).max(1.0 / 16.0);
                        let secs = (p.end - p.start) / (p.base_inc * lowest) / out_rate as f64;
                        map.one_shot_tail = map.one_shot_tail.max(secs as f32).min(MAX_ONE_SHOT_TAIL);
                    }
                    map.zones.push(p);
                }
                Err(error) => issues.push(ZoneIssue { zone: z.id, file: z.file.clone(), error }),
            }
        }
        let mut key_index = Vec::with_capacity(128);
        for key in 0..128u8 {
            let from = map.by_key.len() as u32;
            for (i, z) in map.zones.iter().enumerate() {
                if key >= z.key_lo && key <= z.key_hi {
                    map.by_key.push(i as u32);
                }
            }
            key_index.push((from, map.by_key.len() as u32 - from));
        }
        map.key_index = key_index;
        (map, issues)
    }

    fn play_zone(z: &Zone, base: &Path, out_rate: f64, pool: &SamplePool) -> Result<PlayZone, LoadError> {
        let path = base.join(&z.file);
        let sample = if z.reverse { pool.get_reversed(&path)? } else { pool.get(&path)? };
        let frames = sample.frames as u64;
        if frames == 0 {
            return Err(LoadError::Unreadable("it has no audio".into()));
        }
        // The file's positions, kept in range, with an end of 0 meaning the end of the file.
        let end = if z.end == 0 { frames } else { z.end.min(frames) };
        let start = z.start.min(end.saturating_sub(1));
        // A backwards file's positions count from its other end.
        let flip = |p: u64| frames - p;
        let (start_at, end_at) = if z.reverse { (flip(end), flip(start)) } else { (start, end) };
        let mut looped = None;
        let mut loop_start = 0;
        if z.loop_on {
            let le = if z.loop_end == 0 { end } else { z.loop_end.min(end) };
            let ls = z.loop_start.min(le.saturating_sub(1));
            let (ls_at, le_at) = if z.reverse { (flip(le), flip(ls)) } else { (ls, le) };
            if le_at > ls_at + 1 {
                loop_start = ls_at;
                let key = LoopKey { path, reverse: z.reverse, start: ls_at, end: le_at, xfade: z.xfade, equal_power: z.equal_power };
                let buf = sample.clone();
                looped = Some(pool.baked_loop(key, || LoopBuf::bake(&buf, ls_at as usize, le_at as usize, z.xfade as usize, z.equal_power)));
            }
        }
        // A sample whose first frame is well away from zero would click at the start.
        let hard_start = sample.ch.iter().any(|c| c[GUARD + start_at as usize].abs() > 1e-3);
        let (mut gain_l, mut gain_r) = (1.0, 1.0);
        if z.pan > 0.0 {
            gain_l = 1.0 - z.pan;
        } else {
            gain_r = 1.0 + z.pan;
        }
        let g = 10f32.powf(z.gain_db / 20.0);
        let out = out_rate as f32;
        Ok(PlayZone {
            id: z.id,
            key_lo: z.key_lo,
            key_hi: z.key_hi,
            vel_lo: z.vel_lo,
            vel_hi: z.vel_hi,
            root: z.root as f64 - z.fine as f64 / 100.0,
            base_inc: sample.rate as f64 / out_rate,
            gain_l: gain_l * g,
            gain_r: gain_r * g,
            looped,
            start: start_at as f64,
            end: end_at as f64,
            loop_start: loop_start as f64,
            one_shot: z.one_shot,
            rr_group: z.rr_group,
            env: z.env.map(|e| EnvShape::new(e[0], e[1], e[2], e[3], out)),
            hard_start,
            sample,
        })
    }

    /// The zones on a key, as indices into `zones`.
    pub fn zones_for_key(&self, key: u8) -> &[u32] {
        let (from, len) = self.key_index.get(key as usize).copied().unwrap_or((0, 0));
        &self.by_key[from as usize..(from + len) as usize]
    }
}
