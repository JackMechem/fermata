//! The operator envelope: four rates and four levels, the way the instrument's envelope chip
//! behaves. The level is a logarithm, so a "decay" at a steady rate is a steady fall in dB.
//!
//! The scales here follow the published reverse engineering of the instrument (Ken Shirriff's die
//! photos, and the fits in Raph Levien's MSFA engine that Dexed grew from): units of 1/256 of a
//! doubling, a rising segment that eases in towards a point above the top, and qrate steps of an
//! octave of speed per 4. Nothing in this file has been measured from an instrument.

/// A level's scale on the panel's 0..99, to the envelope chip's units.
const LOW_LEVELS: [i32; 20] = [0, 5, 9, 13, 17, 20, 23, 25, 27, 29, 31, 33, 35, 37, 39, 41, 42, 43, 45, 46];

pub fn scale_out_level(l: i32) -> i32 {
    let l = l.clamp(0, 99);
    if l >= 20 {
        28 + l
    } else {
        LOW_LEVELS[l as usize]
    }
}

/// Where a rising segment heads for, in level units.
const ATTACK_LIMIT: f32 = 4352.0;
/// A rising segment starts at least this far up: the jump in a note's attack.
const ATTACK_JUMP: f32 = 1716.0;
/// The lowest level there is.
const FLOOR: i32 = 16;
/// The stretch of the scale between "on" and silent.
pub const TOP: f32 = 3840.0;

/// How the envelope runs on: the block's ticks at the instrument's rate, against the 44.1 kHz with
/// 64-tick blocks that the published fits are at.
const BLOCK_SCALE: f32 = 44100.0 / super::DX_RATE as f32;

/// One operator's envelope in one voice.
#[derive(Debug, Clone, Copy, Default)]
pub struct DxEnv {
    pub rates: [i32; 4],
    pub levels: [i32; 4],
    /// The operator's level scale, already with its velocity and keyboard scaling.
    pub out_level: i32,
    pub rate_scale: i32,
    pub level: f32,
    target: f32,
    rising: bool,
    /// Per block.
    inc: f32,
    /// The stage: 0 to 2 with the key down, holding at 3 until it comes up, 4 is the end.
    pub ix: usize,
    down: bool,
}

impl DxEnv {
    /// Key down: runs from wherever the level is now.
    pub fn key_on(&mut self) {
        self.down = true;
        self.advance(0);
    }

    pub fn key_off(&mut self) {
        if self.down {
            self.down = false;
            self.advance(3);
        }
    }

    /// The panel moved under a held note: it heads for the new levels from where it is, at the new
    /// rates. A level holding at the third goes back through that segment, so a knob or a lane is
    /// heard on the note it's moving, at the envelope's own speed.
    pub fn retarget(&mut self, rates: [i32; 4], levels: [i32; 4], out_level: i32, rate_scale: i32) {
        self.rates = rates;
        self.levels = levels;
        self.out_level = out_level;
        self.rate_scale = rate_scale;
        if self.ix <= 2 {
            self.advance(self.ix);
        } else if self.ix == 3 {
            self.advance(if self.down { 2 } else { 3 });
        }
    }

    /// Whether it has run to its end, at its fourth level.
    pub fn finished(&self) -> bool {
        self.ix >= 4
    }

    fn advance(&mut self, ix: usize) {
        self.ix = ix;
        if ix < 4 {
            let actual = scale_out_level(self.levels[ix]) >> 1;
            let actual = ((actual << 6) + self.out_level - 4256).max(FLOOR);
            self.target = actual as f32;
            self.rising = self.target > self.level;
            let q = ((self.rates[ix] * 41) >> 6) + self.rate_scale;
            let q = q.min(63);
            // 2^(8 + q/4) in the fixed point the fits use, over 2^16 for a level unit.
            self.inc = (4 + (q & 3)) as f32 * ((2 + 6 + (q >> 2)) as f32).exp2() / 65536.0 * BLOCK_SCALE;
        }
    }

    /// Moves on one block of 64 ticks. Returns the level.
    #[inline]
    pub fn block(&mut self) -> f32 {
        if self.ix < 3 || (self.ix < 4 && !self.down) {
            if self.rising {
                if self.level < ATTACK_JUMP {
                    self.level = ATTACK_JUMP;
                }
                self.level += (ATTACK_LIMIT - self.level) / 256.0 * self.inc;
                if self.level >= self.target {
                    self.level = self.target;
                    self.advance(self.ix + 1);
                }
            } else {
                self.level -= self.inc;
                if self.level <= self.target {
                    self.level = self.target;
                    self.advance(self.ix + 1);
                }
            }
        }
        self.level
    }
}

/// How long a release at this rate takes from the top, in seconds: for a tail.
pub fn release_secs(rate: i32) -> f32 {
    let q = ((rate * 41) >> 6).min(63);
    let inc = (4 + (q & 3)) as f32 * ((8 + (q >> 2)) as f32).exp2() / 65536.0 * BLOCK_SCALE;
    TOP / inc.max(1e-3) * 64.0 / super::DX_RATE as f32
}

/// How velocity moves an operator's level, for its sensitivity 0..7. The instrument has a table for
/// the curve; this is a fit to the published one's shape: 0 at velocity 100, about -3300 units at
/// velocity 0 and full sensitivity. Close, not the same.
pub fn velocity_scale(velocity: f32, sens: i32) -> i32 {
    let v = velocity.clamp(0.0, 1.0);
    let shaped = (v.powf(0.32) / (100.0f32 / 127.0).powf(0.32) - 1.0) * 239.0;
    (((sens as f32 * shaped) / 8.0).round() as i32) << 4
}

/// The keyboard level scaling's exponential curve, by distance in thirds of an octave (a published
/// table, from the same reverse engineering).
const EXP_CURVE: [i32; 33] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 11, 14, 16, 19, 23, 27, 33, 39, 47, 56, 66, 80, 94, 110, 126, 142, 158, 174, 190, 206, 222, 238, 250];

fn scale_curve(group: i32, depth: i32, curve: i32) -> i32 {
    let scale = if curve == 0 || curve == 3 {
        (group * depth * 329) >> 12
    } else {
        let raw = EXP_CURVE[group.clamp(0, 32) as usize];
        (raw * depth * 329) >> 15
    };
    if curve < 2 {
        -scale
    } else {
        scale
    }
}

/// How the keyboard moves an operator's level: `break_point` is on the panel's 0..99, where 39 is
/// middle C.
pub fn keyboard_level_scale(note: i32, break_point: i32, l_depth: i32, r_depth: i32, l_curve: i32, r_curve: i32) -> i32 {
    let offset = note - (break_point + 21);
    if offset >= 0 {
        scale_curve((offset + 1) / 3, r_depth, r_curve)
    } else {
        scale_curve((1 - offset) / 3, l_depth, l_curve)
    }
}

/// How the keyboard speeds an envelope up, in qrate steps.
pub fn keyboard_rate_scale(note: i32, sens: i32) -> i32 {
    let x = (note / 3 - 7).clamp(0, 31);
    (sens * x) >> 3
}

/// Pitch envelope: a level on the panel's 0..99 to semitones. 50 is no shift. The published table
/// is near a straight line in the middle and steepens to ±4 octaves at the ends: this is that
/// shape by its corners.
pub fn pitch_level_st(l: f32) -> f32 {
    const POINTS: [(f32, f32); 6] = [(0.0, 48.0), (5.0, 28.5), (10.0, 18.4), (15.0, 13.9), (20.0, 11.25), (50.0, 0.0)];
    let x = (l - 50.0).abs();
    let at = 50.0 - x;
    let mut st = 0.0;
    for w in POINTS.windows(2) {
        if at >= w[0].0 && at <= w[1].0 {
            st = w[0].1 + (w[1].1 - w[0].1) * (at - w[0].0) / (w[1].0 - w[0].0);
            break;
        }
    }
    if l >= 50.0 {
        st
    } else {
        -st
    }
}

/// Pitch envelope: a rate on the panel's 0..99, to octaves per second. A fit to the published table.
pub fn pitch_rate_oct_per_sec(r: f32) -> f32 {
    (0.0556 * r).exp() / 21.3
}

/// The LFO's speed, on the panel's 0..99, in Hz: the published formula.
pub fn lfo_hz(speed: i32) -> f32 {
    let s = if speed <= 0 { 1 } else { (165 * speed) >> 6 };
    let s = s * if s < 160 { 11 } else { 11 + ((s - 160) >> 4) };
    s as f32 * 25_190_424.0 / 4_294_967_296.0
}
