//! The Compressor, modelled on Logic Pro's: one set of controls in front of five circuit types,
//! each with the gain-reduction behaviour of the hardware it stands for.
//!
//! - **Platinum Digital**: a clean feedforward compressor working in the log domain, with the
//!   smooth-branching detector and quadratic soft knee from Giannoulis, Massberg & Reiss,
//!   "Digital Dynamic Range Compressor Design — A Tutorial and Analysis" (JAES 2012). The
//!   threshold, ratio, knee and time constants mean exactly what they say.
//! - **Studio VCA** (SSL G-bus, dbx 160): the same feedforward VCA topology with the console's
//!   stepped attack (0.1–30 ms) and release (0.1–1.2 s) positions, plus Auto release: a
//!   dual-time-constant release that recovers fast after transients and slowly after sustained
//!   compression.
//! - **Studio FET** (UREI 1176): a feedback compressor. The detector listens to the compressed
//!   output, and the loop is solved without delay each sample (Newton's method on the
//!   implicit equation), so it stays stable with its 20 µs attack. The ratio buttons are
//!   4/8/12/20:1 plus "All buttons in", whose bias shift lowers the threshold, slows the attack
//!   and over-compresses: above a point, louder input gives quieter output. The release is
//!   program-dependent (longer after heavy compression), and a FET-style odd-harmonic colour
//!   grows with gain reduction.
//! - **Classic Opto** (Teletronix LA-2A): an optical cell with a ~10 ms attack and the T4's
//!   two-stage release: about half the reduction recovers in ~60 ms, the rest over 1–15 s,
//!   depending on how long and how hard the cell was lit. Its soft knee has a ratio that keeps
//!   rising with level (Compress tops out near 3:1, Limit near 12:1). The panel shows the
//!   threshold as Peak Reduction.
//! - **Vintage Vari-Mu** (Fairchild 670, Manley): a variable-mu tube with a very wide knee
//!   whose ratio rises with level towards the Ratio setting, the 670's six time-constant
//!   positions (5 and 6 are program-dependent), and even-harmonic tube colour that grows with
//!   gain reduction.
//!
//! All circuits share the signal path around them: side-chain key or input → side-chain filter
//! → detector (peak or RMS) → stereo link → gain computer and ballistics → lookahead-delayed
//! audio × gain → circuit colour → makeup and auto gain → output distortion → dry/wet mix →
//! output gain. Gain reduction is computed in dB, so the envelopes behave the same at every
//! level.
//!
//! Provenance: written for Fermata from the papers and published behaviour of the units above;
//! no third-party code is included. The idea of letting the FET release slow down as reduction
//! builds up was suggested by the program-dependent "speed" variable in Airwindows' Pop2
//! (MIT licence, Copyright (c) 2016 airwindows), but none of its code is used.

use super::dyn_common::{
    db_to_lin, flush, knee_reduction, knee_reduction_slope, lin_to_db, program_reduction, DcBlock, Delay,
    Distortion, ScMode, Smoother, Svf,
};
use super::util::coeff;
use super::{Effect, EffectInfo, FactoryPreset, FxContext, Key, ParamKind, ParamSpec, Unit, MAX_METERS};

/// The plug-in id.
pub const ID: &str = "builtin:compressor";

/// Parameter ids (stable forever: saved projects and automation refer to them).
pub mod param {
    pub const CIRCUIT: u32 = 0;
    pub const THRESHOLD: u32 = 1;
    pub const RATIO: u32 = 2;
    pub const KNEE: u32 = 3;
    pub const ATTACK: u32 = 4;
    pub const RELEASE: u32 = 5;
    pub const AUTO_RELEASE: u32 = 6;
    pub const MAKEUP: u32 = 7;
    pub const AUTO_GAIN: u32 = 8;
    pub const DETECTOR: u32 = 9;
    pub const DISTORTION: u32 = 10;
    pub const MIX: u32 = 11;
    pub const OUTPUT: u32 = 12;
    pub const LINK: u32 = 13;
    pub const SC_MODE: u32 = 14;
    pub const SC_FREQ: u32 = 15;
    pub const LISTEN: u32 = 16;
    pub const LOOKAHEAD: u32 = 17;
    pub const FET_RATIO: u32 = 18;
    pub const OPTO_MODE: u32 = 19;
    pub const TIME_CONSTANT: u32 = 20;
}

const NPARAMS: usize = 21;

const fn spec(id: u32, name: &'static str, min: f32, max: f32, default: f32, unit: Unit, kind: ParamKind, log: bool) -> ParamSpec {
    ParamSpec { id, name, min, max, default, unit, kind, log }
}

const C: ParamKind = ParamKind::Continuous;
const T: ParamKind = ParamKind::Toggle;

static PARAMS: [ParamSpec; NPARAMS] = [
    spec(
        param::CIRCUIT,
        "Circuit",
        0.0,
        4.0,
        0.0,
        Unit::None,
        ParamKind::Choice(&["Platinum Digital", "Studio VCA", "Studio FET", "Classic Opto", "Vintage Vari-Mu"]),
        false,
    ),
    spec(param::THRESHOLD, "Threshold", -60.0, 0.0, -20.0, Unit::Db, C, false),
    spec(param::RATIO, "Ratio", 1.0, 30.0, 4.0, Unit::Ratio, C, true),
    spec(param::KNEE, "Knee", 0.0, 24.0, 3.0, Unit::Db, C, false),
    spec(param::ATTACK, "Attack", 0.02, 200.0, 10.0, Unit::Ms, C, true),
    spec(param::RELEASE, "Release", 5.0, 5000.0, 150.0, Unit::Ms, C, true),
    spec(param::AUTO_RELEASE, "Auto Release", 0.0, 1.0, 0.0, Unit::None, T, false),
    spec(param::MAKEUP, "Make Up", -20.0, 40.0, 0.0, Unit::Db, C, false),
    spec(param::AUTO_GAIN, "Auto Gain", 0.0, 3.0, 0.0, Unit::None, ParamKind::Choice(&["Off", "0 dB", "-12 dB", "-24 dB"]), false),
    spec(param::DETECTOR, "Detection", 0.0, 1.0, 0.0, Unit::None, ParamKind::Choice(&["Peak", "RMS"]), false),
    spec(param::DISTORTION, "Distortion", 0.0, 3.0, 0.0, Unit::None, ParamKind::Choice(&["Off", "Soft", "Hard", "Clip"]), false),
    spec(param::MIX, "Mix", 0.0, 1.0, 1.0, Unit::Percent, C, false),
    spec(param::OUTPUT, "Output", -30.0, 12.0, 0.0, Unit::Db, C, false),
    spec(param::LINK, "Stereo Link", 0.0, 1.0, 1.0, Unit::Percent, C, false),
    spec(param::SC_MODE, "Side Chain Filter", 0.0, 3.0, 0.0, Unit::None, ParamKind::Choice(&["Off", "HP", "LP", "BP"]), false),
    spec(param::SC_FREQ, "Side Chain Freq", 20.0, 20000.0, 150.0, Unit::Hz, C, true),
    spec(param::LISTEN, "Listen", 0.0, 1.0, 0.0, Unit::None, T, false),
    spec(param::LOOKAHEAD, "Lookahead", 0.0, 20.0, 0.0, Unit::Ms, C, false),
    spec(param::FET_RATIO, "FET Ratio", 0.0, 4.0, 0.0, Unit::None, ParamKind::Choice(&["4:1", "8:1", "12:1", "20:1", "All"]), false),
    spec(param::OPTO_MODE, "Opto Mode", 0.0, 1.0, 0.0, Unit::None, ParamKind::Choice(&["Compress", "Limit"]), false),
    spec(
        param::TIME_CONSTANT,
        "Time Constant",
        0.0,
        5.0,
        0.0,
        Unit::None,
        ParamKind::Choice(&["1 (0.3 s)", "2 (0.8 s)", "3 (2 s)", "4 (5 s)", "5 (2–10 s)", "6 (0.3–25 s)"]),
        false,
    ),
];

/// Factory presets. Each lists only what it changes from the defaults.
pub const PRESETS: &[FactoryPreset] = {
    use param::*;
    &[
        FactoryPreset {
            name: "Vocal Leveler (Opto)",
            values: &[(CIRCUIT, 3.0), (THRESHOLD, -24.0), (OPTO_MODE, 0.0), (MAKEUP, 5.0)],
        },
        FactoryPreset {
            name: "Vocal Aggressive (FET)",
            values: &[(CIRCUIT, 2.0), (THRESHOLD, -26.0), (FET_RATIO, 1.0), (ATTACK, 0.2), (RELEASE, 120.0), (MAKEUP, 7.0)],
        },
        FactoryPreset {
            name: "Drum Bus Glue (VCA)",
            values: &[
                (CIRCUIT, 1.0),
                (THRESHOLD, -18.0),
                (RATIO, 4.0),
                (KNEE, 2.0),
                (ATTACK, 10.0),
                (RELEASE, 100.0),
                (DETECTOR, 1.0),
                (MAKEUP, 3.0),
                (SC_MODE, 1.0),
                (SC_FREQ, 80.0),
            ],
        },
        FactoryPreset {
            name: "Mix Bus Glue (VCA)",
            values: &[
                (CIRCUIT, 1.0),
                (THRESHOLD, -14.0),
                (RATIO, 2.0),
                (KNEE, 4.0),
                (ATTACK, 30.0),
                (AUTO_RELEASE, 1.0),
                (DETECTOR, 1.0),
                (MAKEUP, 1.5),
                (SC_MODE, 1.0),
                (SC_FREQ, 60.0),
            ],
        },
        FactoryPreset {
            // A slow attack lets the beater through before the VCA clamps down.
            name: "Kick Punch (VCA)",
            values: &[(CIRCUIT, 1.0), (THRESHOLD, -16.0), (RATIO, 4.0), (KNEE, 0.0), (ATTACK, 30.0), (RELEASE, 100.0), (MAKEUP, 3.0)],
        },
        FactoryPreset {
            name: "Snare Smack (FET)",
            values: &[(CIRCUIT, 2.0), (THRESHOLD, -24.0), (FET_RATIO, 0.0), (ATTACK, 0.8), (RELEASE, 60.0), (MAKEUP, 5.0)],
        },
        FactoryPreset {
            name: "1176 All Buttons",
            values: &[(CIRCUIT, 2.0), (FET_RATIO, 4.0), (THRESHOLD, -30.0), (ATTACK, 0.8), (RELEASE, 60.0), (MAKEUP, 6.0)],
        },
        FactoryPreset {
            name: "Bass Even Out (Opto)",
            values: &[(CIRCUIT, 3.0), (THRESHOLD, -22.0), (OPTO_MODE, 0.0), (MAKEUP, 4.0), (SC_MODE, 1.0), (SC_FREQ, 40.0)],
        },
        FactoryPreset {
            name: "Acoustic Gentle (Platinum)",
            values: &[
                (THRESHOLD, -22.0),
                (RATIO, 2.0),
                (KNEE, 9.0),
                (ATTACK, 15.0),
                (RELEASE, 250.0),
                (AUTO_RELEASE, 1.0),
                (DETECTOR, 1.0),
                (MAKEUP, 2.0),
            ],
        },
        FactoryPreset {
            // Crush hard, then blend under the dry drums (New York compression).
            name: "Parallel Drums (Mix 40 %)",
            values: &[
                (CIRCUIT, 2.0),
                (FET_RATIO, 3.0),
                (THRESHOLD, -35.0),
                (ATTACK, 0.1),
                (RELEASE, 150.0),
                (MAKEUP, 10.0),
                (DISTORTION, 1.0),
                (MIX, 0.4),
            ],
        },
        FactoryPreset {
            name: "Fairchild Warmth (Vari-Mu)",
            values: &[(CIRCUIT, 4.0), (THRESHOLD, -20.0), (RATIO, 3.0), (TIME_CONSTANT, 1.0), (MAKEUP, 3.0)],
        },
        FactoryPreset {
            name: "Vari-Mu Bus (Program TC 5)",
            values: &[(CIRCUIT, 4.0), (THRESHOLD, -16.0), (RATIO, 2.0), (TIME_CONSTANT, 4.0), (MAKEUP, 1.5)],
        },
        FactoryPreset {
            name: "Transparent Peak Control (Platinum)",
            values: &[(THRESHOLD, -10.0), (RATIO, 8.0), (KNEE, 2.0), (ATTACK, 1.0), (RELEASE, 80.0), (LOOKAHEAD, 2.0)],
        },
        FactoryPreset {
            // Pick the kick (or voice-over) track as the side-chain source; the low-pass keeps
            // the detector on its low end.
            name: "Side-chain Duck",
            values: &[(THRESHOLD, -30.0), (RATIO, 6.0), (KNEE, 3.0), (ATTACK, 2.0), (RELEASE, 180.0), (SC_MODE, 2.0), (SC_FREQ, 150.0)],
        },
    ]
};

/// Meter names, in the order [`Effect::meters`] writes them. Input and Output are peak dBFS,
/// Gain Reduction is positive dB, and Level is the detector's level in dB (the x position of
/// the dot on the panel's transfer curve).
pub const METERS: &[&str] = &["Input", "Output", "Gain Reduction", "Level"];

/// The registry entry.
pub fn info() -> EffectInfo {
    EffectInfo {
        id: ID,
        name: "Compressor",
        category: "Dynamics",
        params: &PARAMS,
        meters: METERS,
        sidechain: true,
        new: |sr| Box::new(Compressor::new(sr)),
    }
}

/// The circuit types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Circuit {
    Platinum,
    Vca,
    Fet,
    Opto,
    VariMu,
}

impl Circuit {
    pub fn from_index(i: f32) -> Circuit {
        match i.round() as i32 {
            1 => Circuit::Vca,
            2 => Circuit::Fet,
            3 => Circuit::Opto,
            4 => Circuit::VariMu,
            _ => Circuit::Platinum,
        }
    }
}

/// The SSL bus compressor's attack positions, in ms. The VCA circuit snaps to the nearest.
pub const VCA_ATTACKS: [f32; 6] = [0.1, 0.3, 1.0, 3.0, 10.0, 30.0];
/// The SSL bus compressor's release positions, in ms (Auto is the Auto Release switch).
pub const VCA_RELEASES: [f32; 4] = [100.0, 300.0, 600.0, 1200.0];
/// The 1176's attack range, in ms.
pub const FET_ATTACK: (f32, f32) = (0.02, 0.8);
/// The 1176's release range, in ms.
pub const FET_RELEASE: (f32, f32) = (50.0, 1100.0);

/// One Fairchild 670 time-constant position: attack, then release stages in ms. Positions 5
/// and 6 add slower stages that charge only under repeated or sustained reduction.
struct TimeConstant {
    attack: f32,
    release: f32,
    multi: Option<f32>,
    sustained: Option<f32>,
}

const VARI_MU_TC: [TimeConstant; 6] = [
    TimeConstant { attack: 0.2, release: 300.0, multi: None, sustained: None },
    TimeConstant { attack: 0.2, release: 800.0, multi: None, sustained: None },
    TimeConstant { attack: 0.4, release: 2000.0, multi: None, sustained: None },
    TimeConstant { attack: 0.4, release: 5000.0, multi: None, sustained: None },
    TimeConstant { attack: 0.4, release: 2000.0, multi: Some(10000.0), sustained: None },
    TimeConstant { attack: 0.2, release: 300.0, multi: Some(10000.0), sustained: Some(25000.0) },
];

/// Snaps a time to the nearest of a set of positions, in log terms (like a stepped switch).
fn nearest(ms: f32, steps: &[f32]) -> f32 {
    let mut best = steps[0];
    for &s in steps {
        if (ms.ln() - s.ln()).abs() < (ms.ln() - best.ln()).abs() {
            best = s;
        }
    }
    best
}

/// The FET ratio buttons: (feedback slope S = R - 1, knee width on the input side in dB).
fn fet_curve(mode: usize) -> (f32, f32) {
    match mode {
        0 => (3.0, 8.0),
        1 => (7.0, 6.0),
        2 => (11.0, 5.0),
        _ => (19.0, 4.0),
    }
}

/// All-buttons-in: how far it lowers the threshold (dB), where its over-compression starts
/// above the threshold (dB), and how many dB of extra reduction it adds per dB beyond that.
/// More than 1 dB/dB is what makes the output fall as the input rises.
const ABI_SHIFT: f32 = 3.0;
const ABI_KNEE: f32 = 4.0;
const ABI_FOLD: f32 = 1.15;

/// The opto curve for Compress / Limit: (slope towards the top ratio, softness in dB).
fn opto_curve(limit: bool) -> (f32, f32) {
    if limit {
        (1.0 - 1.0 / 12.0, 3.0)
    } else {
        (1.0 - 1.0 / 3.0, 5.0)
    }
}

/// How soft the vari-mu curve is: its ratio rises over about ±2 widths around the threshold.
const VARI_MU_WIDTH: f32 = 8.0;

/// The settings that shape the static curve, gathered so the curve can be evaluated alone.
#[derive(Debug, Clone, Copy)]
pub struct CurveParams {
    pub circuit: Circuit,
    pub threshold: f32,
    pub ratio: f32,
    pub knee: f32,
    pub fet_mode: usize,
    pub opto_limit: bool,
}

impl CurveParams {
    /// The FET's feedback slope and knee in the output domain. A knee `k` dB wide on the
    /// input side is `2k / (S + 2)` wide on the output side the detector sees.
    fn fet(&self) -> (f32, f32, bool) {
        let abi = self.fet_mode >= 4;
        let (s, k_in) = fet_curve(self.fet_mode);
        (s, 2.0 * k_in / (s + 2.0), abi)
    }

    fn fet_threshold(&self) -> f32 {
        if self.fet_mode >= 4 {
            self.threshold - ABI_SHIFT
        } else {
            self.threshold
        }
    }
}

/// All-buttons-in over-compression added to the FET target for an input `xo` dB over the
/// (shifted) threshold.
#[inline]
fn abi_extra(xo: f32) -> f32 {
    ABI_FOLD * (xo - ABI_KNEE).max(0.0)
}

/// The steady-state gain reduction in dB for a constant detector level `x` dB: what the
/// compressor settles to, and what Auto Gain compensates.
pub fn static_reduction(p: &CurveParams, x: f32) -> f32 {
    match p.circuit {
        Circuit::Platinum | Circuit::Vca => knee_reduction(x - p.threshold, 1.0 - 1.0 / p.ratio.max(1.0), p.knee),
        Circuit::Fet => {
            // Solve c = S·knee(x - T - c) (+ all-buttons fold) for the reduction c.
            let (s, kn, abi) = p.fet();
            let xo = x - p.fet_threshold();
            let extra = if abi { abi_extra(xo) } else { 0.0 };
            let mut c = 0.0f32;
            for _ in 0..30 {
                let f = c - knee_reduction(xo - c, s, kn) - extra;
                let fp = 1.0 + knee_reduction_slope(xo - c, s, kn);
                c -= f / fp;
            }
            c.max(0.0)
        }
        Circuit::Opto => {
            let (slope, w) = opto_curve(p.opto_limit);
            program_reduction(x - p.threshold, slope, w)
        }
        Circuit::VariMu => program_reduction(x - p.threshold, 1.0 - 1.0 / p.ratio.max(1.0), VARI_MU_WIDTH),
    }
}

/// One channel's detector and envelope state. Reductions are in dB (positive).
#[derive(Debug, Clone, Copy, Default)]
struct Chan {
    /// Mean square, for RMS detection.
    ms: f32,
    /// The main envelope: Platinum/VCA reduction, the FET loop state, the opto's fast
    /// element, the vari-mu's first stage.
    gr: f32,
    /// Auto release's slow envelope, the opto cell's persistence, the vari-mu's second stage.
    slow: f32,
    /// The vari-mu's third stage (position 6).
    slower: f32,
    /// How hard and long the FET or the opto cell has been working (program memory).
    memory: f32,
    dc: DcBlock,
}

/// Per-block constants for the envelopes (time coefficients change slowly, so working them
/// out once per block is enough).
struct Block {
    k_att: f32,
    k_rel: f32,
    k_slow_att: f32,
    k_slow_rel: f32,
    k_slower_att: f32,
    k_slower_rel: f32,
    k_memory: f32,
    k_memory_rel: f32,
    auto_release: bool,
    multi: bool,
    sustained: bool,
    sr: f32,
}

/// The Compressor instance.
pub struct Compressor {
    sr: f32,
    values: [f32; NPARAMS],
    circuit: Circuit,
    threshold: Smoother,
    ratio: Smoother,
    knee: Smoother,
    makeup: Smoother,
    auto_gain: Smoother,
    mix: Smoother,
    output: Smoother,
    link: Smoother,
    svf: Svf,
    sc_set: (ScMode, f32),
    chans: [Chan; 2],
    /// Lookahead delays: audio L/R, then the listened key L/R (so Listen stays aligned).
    delays: [Delay; 4],
    lookahead: usize,
    k_rms: f32,
    m_in: f32,
    m_out: f32,
    m_gr: f32,
    m_level: f32,
}

impl Compressor {
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(8000.0);
        let mut values = [0.0; NPARAMS];
        for p in &PARAMS {
            values[p.id as usize] = p.default;
        }
        let v = |id: u32| values[id as usize];
        let max_delay = (0.020 * sr).ceil() as usize + 1;
        let mut c = Compressor {
            sr,
            values,
            circuit: Circuit::from_index(v(param::CIRCUIT)),
            threshold: Smoother::new(v(param::THRESHOLD), sr),
            ratio: Smoother::new(v(param::RATIO), sr),
            knee: Smoother::new(v(param::KNEE), sr),
            makeup: Smoother::new(v(param::MAKEUP), sr),
            auto_gain: Smoother::new(0.0, sr),
            mix: Smoother::new(v(param::MIX), sr),
            output: Smoother::new(v(param::OUTPUT), sr),
            link: Smoother::new(v(param::LINK), sr),
            svf: Svf::default(),
            sc_set: (ScMode::Off, 0.0),
            chans: [Chan { dc: DcBlock::new(sr), ..Chan::default() }; 2],
            delays: std::array::from_fn(|_| Delay::new(max_delay)),
            lookahead: 0,
            k_rms: coeff(8.0, sr),
            m_in: -120.0,
            m_out: -120.0,
            m_gr: 0.0,
            m_level: -120.0,
        };
        c.update_auto_gain();
        c.auto_gain.snap();
        c
    }

    fn v(&self, id: u32) -> f32 {
        self.values[id as usize]
    }

    fn curve_params(&self) -> CurveParams {
        CurveParams {
            circuit: self.circuit,
            threshold: self.threshold.target(),
            ratio: self.ratio.target(),
            knee: self.knee.target(),
            fet_mode: self.v(param::FET_RATIO).round().clamp(0.0, 4.0) as usize,
            opto_limit: self.v(param::OPTO_MODE) >= 0.5,
        }
    }

    /// Auto Gain makes up the reduction a signal at the chosen reference level would get.
    fn update_auto_gain(&mut self) {
        let reference = match self.v(param::AUTO_GAIN).round() as i32 {
            1 => Some(0.0),
            2 => Some(-12.0),
            3 => Some(-24.0),
            _ => None,
        };
        let db = reference.map_or(0.0, |r| static_reduction(&self.curve_params(), r));
        self.auto_gain.set(db);
    }

    /// The attack and release the current circuit actually uses, in ms, after its own range
    /// or switch positions. The panel mirrors this.
    pub fn effective_times(&self) -> (f32, f32) {
        let a = self.v(param::ATTACK);
        let r = self.v(param::RELEASE);
        match self.circuit {
            Circuit::Platinum => (a, r),
            Circuit::Vca => (nearest(a, &VCA_ATTACKS), nearest(r, &VCA_RELEASES)),
            Circuit::Fet => (a.clamp(FET_ATTACK.0, FET_ATTACK.1), r.clamp(FET_RELEASE.0, FET_RELEASE.1)),
            Circuit::Opto => (10.0, 60.0),
            Circuit::VariMu => {
                let tc = &VARI_MU_TC[self.v(param::TIME_CONSTANT).round().clamp(0.0, 5.0) as usize];
                (tc.attack, tc.release)
            }
        }
    }

    fn block_consts(&self) -> Block {
        let sr = self.sr;
        let (a, r) = self.effective_times();
        let auto = self.v(param::AUTO_RELEASE) >= 0.5;
        let none = Block {
            k_att: coeff(a, sr),
            k_rel: coeff(r, sr),
            k_slow_att: 0.0,
            k_slow_rel: 0.0,
            k_slower_att: 0.0,
            k_slower_rel: 0.0,
            k_memory: 0.0,
            k_memory_rel: 0.0,
            auto_release: false,
            multi: false,
            sustained: false,
            sr,
        };
        match self.circuit {
            // Auto release: a fast stage for transients and a slow one that only charges
            // under sustained reduction. Platinum scales both from the Release knob; the VCA's
            // Auto position has fixed console-style times.
            Circuit::Platinum if auto => Block {
                k_rel: coeff((r * 0.3).max(10.0), sr),
                k_slow_att: coeff(300.0, sr),
                k_slow_rel: coeff(r * 3.0, sr),
                auto_release: true,
                ..none
            },
            Circuit::Vca if auto => Block {
                k_rel: coeff(100.0, sr),
                k_slow_att: coeff(300.0, sr),
                k_slow_rel: coeff(1200.0, sr),
                auto_release: true,
                ..none
            },
            Circuit::Platinum | Circuit::Vca => none,
            Circuit::Fet => {
                // All buttons in slows the attack, which lets the transients through.
                let a = if self.curve_params().fet_mode >= 4 { a * 3.0 } else { a };
                Block { k_att: coeff(a, sr), k_memory: coeff(400.0, sr), ..none }
            }
            Circuit::Opto => Block {
                // The cell's persistence charges over about a second; its memory of hard work
                // builds over a few seconds and fades over half a minute (the slow release
                // itself depends on that memory, so it is worked out per sample).
                k_slow_att: coeff(1000.0, sr),
                k_memory: coeff(3000.0, sr),
                k_memory_rel: coeff(30000.0, sr),
                ..none
            },
            Circuit::VariMu => {
                let tc = &VARI_MU_TC[self.v(param::TIME_CONSTANT).round().clamp(0.0, 5.0) as usize];
                Block {
                    // Repeated peaks within a fraction of a second charge the multi-peak stage;
                    // only seconds of sustained level charge the slowest one.
                    k_slow_att: coeff(150.0, sr),
                    k_slow_rel: tc.multi.map_or(0.0, |ms| coeff(ms, sr)),
                    k_slower_att: coeff(2000.0, sr),
                    k_slower_rel: tc.sustained.map_or(0.0, |ms| coeff(ms, sr)),
                    multi: tc.multi.is_some(),
                    sustained: tc.sustained.is_some(),
                    ..none
                }
            }
        }
    }

    /// Advances one channel's envelope for a detector level `x` (dB) and returns its gain
    /// reduction in dB.
    #[inline]
    fn reduce(ch: &mut Chan, x: f32, cp: &CurveParams, b: &Block) -> f32 {
        match cp.circuit {
            Circuit::Platinum | Circuit::Vca => {
                // Smooth branching peak detector in the log domain (Giannoulis et al., §3.2).
                let t = knee_reduction(x - cp.threshold, 1.0 - 1.0 / cp.ratio, cp.knee);
                let k = if t > ch.gr { b.k_att } else { b.k_rel };
                ch.gr = flush(ch.gr + k * (t - ch.gr));
                if b.auto_release {
                    let ks = if t > ch.slow { b.k_slow_att } else { b.k_slow_rel };
                    ch.slow = flush(ch.slow + ks * (t - ch.slow));
                    ch.gr.max(0.75 * ch.slow)
                } else {
                    ch.gr
                }
            }
            Circuit::Fet => {
                // Feedback: the target depends on the output level x - c, which depends on the
                // reduction c being solved for. c = c₀ + k·(t(x - c) - c₀) is monotonic in c,
                // so a few Newton steps from c₀ find it; the loop can't ring however fast the
                // attack.
                let (s, kn, abi) = cp.fet();
                let xo = x - cp.fet_threshold();
                let extra = if abi { abi_extra(xo) } else { 0.0 };
                let c0 = ch.gr;
                let attacking = knee_reduction(xo - c0, s, kn) + extra > c0;
                let k = if attacking {
                    b.k_att
                } else {
                    // Program-dependent release: after heavy, sustained reduction the FET
                    // recovers up to 2.5× slower. k ≈ 1/(τ·fs) at these lengths, so dividing
                    // k stretches τ.
                    b.k_rel / (1.0 + 1.5 * (ch.memory / 12.0).min(1.0))
                };
                let mut c = c0;
                for _ in 0..4 {
                    let f = c - c0 - k * (knee_reduction(xo - c, s, kn) + extra - c0);
                    let fp = 1.0 + k * knee_reduction_slope(xo - c, s, kn);
                    c -= f / fp;
                }
                ch.gr = flush(c.max(0.0));
                ch.memory = flush(ch.memory + b.k_memory * (ch.gr - ch.memory));
                ch.gr
            }
            Circuit::Opto => {
                let (slope, w) = opto_curve(cp.opto_limit);
                let t = program_reduction(x - cp.threshold, slope, w);
                // The fast part of the cell: ~10 ms attack, ~60 ms release.
                let k = if t > ch.gr { b.k_att } else { b.k_rel };
                ch.gr = flush(ch.gr + k * (t - ch.gr));
                // The slow part: charges over about a second of reduction and lets go over
                // 1 s (briefly lit) to 15 s (worked hard for a long time).
                let ks = if t > ch.slow { b.k_slow_att } else { 1.0 / ((1.0 + 14.0 * ch.memory) * b.sr) };
                ch.slow = flush(ch.slow + ks * (t - ch.slow));
                let lit = (t / 12.0).min(1.0);
                let km = if lit > ch.memory { b.k_memory } else { b.k_memory_rel };
                ch.memory = flush(ch.memory + km * (lit - ch.memory));
                ch.gr.max(0.5 * ch.slow)
            }
            Circuit::VariMu => {
                let t = program_reduction(x - cp.threshold, 1.0 - 1.0 / cp.ratio, VARI_MU_WIDTH);
                let k = if t > ch.gr { b.k_att } else { b.k_rel };
                ch.gr = flush(ch.gr + k * (t - ch.gr));
                let mut gr = ch.gr;
                if b.multi {
                    let ks = if t > ch.slow { b.k_slow_att } else { b.k_slow_rel };
                    ch.slow = flush(ch.slow + ks * (t - ch.slow));
                    gr = gr.max(ch.slow);
                }
                if b.sustained {
                    let ks = if t > ch.slower { b.k_slower_att } else { b.k_slower_rel };
                    ch.slower = flush(ch.slower + ks * (t - ch.slower));
                    gr = gr.max(ch.slower);
                }
                gr
            }
        }
    }

    /// The circuit's own colour on the gain-reduced signal (before makeup).
    #[inline]
    fn colour(ch: &mut Chan, circuit: Circuit, abi: bool, gr: f32, y: f32) -> f32 {
        match circuit {
            Circuit::Platinum | Circuit::Vca => y,
            Circuit::Fet => {
                // Odd harmonics from the FET, stronger the harder it works (and much stronger
                // with all buttons in). tanh(y) ≈ y - y³/3, so the colour also grows with level.
                let a = if abi { (0.3 + 0.05 * gr).min(1.0) } else { (0.12 + 0.03 * gr).min(0.8) };
                y + a * (y.tanh() - y)
            }
            Circuit::Opto => {
                // The LA-2A's tube make-up stage: a faint, constant second harmonic.
                let t = y.tanh();
                ch.dc.process(y + 0.01 * t * t)
            }
            Circuit::VariMu => {
                // Even harmonics from the remote-cutoff tube, rising with reduction.
                let b = (0.02 + 0.006 * gr).min(0.15);
                let t = y.tanh();
                ch.dc.process(y + b * t * t)
            }
        }
    }
}

impl Effect for Compressor {
    fn set_param(&mut self, id: u32, value: f32) {
        let Some(slot) = self.values.get_mut(id as usize) else { return };
        *slot = value;
        match id {
            param::CIRCUIT => {
                let c = Circuit::from_index(value);
                if c != self.circuit {
                    // Each circuit's envelopes mean different things; start the new one clean.
                    self.circuit = c;
                    for ch in &mut self.chans {
                        *ch = Chan { dc: ch.dc, ..Chan::default() };
                    }
                }
            }
            param::THRESHOLD => self.threshold.set(value),
            param::RATIO => self.ratio.set(value.max(1.0)),
            param::KNEE => self.knee.set(value.max(0.0)),
            param::MAKEUP => self.makeup.set(value),
            param::MIX => self.mix.set(value.clamp(0.0, 1.0)),
            param::OUTPUT => self.output.set(value),
            param::LINK => self.link.set(value.clamp(0.0, 1.0)),
            param::LOOKAHEAD => {
                self.lookahead = (value.max(0.0) * 0.001 * self.sr).round() as usize;
                for d in &mut self.delays {
                    d.set_len(self.lookahead);
                }
            }
            _ => {}
        }
        self.update_auto_gain();
    }

    fn param(&self, id: u32) -> f32 {
        self.values.get(id as usize).copied().unwrap_or(0.0)
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], key: Key, _ctx: &FxContext) {
        let n = l.len().min(r.len());
        let b = self.block_consts();
        let circuit = self.circuit;
        let fet_mode = self.curve_params().fet_mode;
        let abi = circuit == Circuit::Fet && fet_mode >= 4;
        let opto_limit = self.v(param::OPTO_MODE) >= 0.5;
        // Platinum and VCA follow the Detection switch; the others are peak-sensing by design
        // (their own ballistics do the averaging).
        let rms = matches!(circuit, Circuit::Platinum | Circuit::Vca) && self.v(param::DETECTOR) >= 0.5;
        let distortion = Distortion::from_index(self.v(param::DISTORTION));
        let listen = self.v(param::LISTEN) >= 0.5;

        let sc_mode = ScMode::from_index(self.v(param::SC_MODE));
        let sc_freq = self.v(param::SC_FREQ);
        if (sc_mode, sc_freq) != self.sc_set {
            let q = if sc_mode == ScMode::BandPass { 1.4 } else { std::f32::consts::FRAC_1_SQRT_2 };
            self.svf.set(sc_freq, q, self.sr);
            self.sc_set = (sc_mode, sc_freq);
        }

        let mut m_in = 0.0f32;
        let mut m_out = 0.0f32;
        let mut m_gr = 0.0f32;
        let mut m_level = -120.0f32;
        for i in 0..n {
            let cp = CurveParams {
                circuit,
                threshold: self.threshold.next(),
                ratio: self.ratio.next(),
                knee: self.knee.next(),
                fet_mode,
                opto_limit,
            };
            let link = self.link.next();
            let gain_db = self.makeup.next() + self.auto_gain.next();
            let mix = self.mix.next();
            let out_gain = db_to_lin(self.output.next());

            let (xl, xr) = (l[i], r[i]);
            m_in = m_in.max(xl.abs()).max(xr.abs());
            let (kl, kr) = match key {
                Some((a, b)) if i < a.len() && i < b.len() => (a[i], b[i]),
                _ => (xl, xr),
            };
            let dl = self.svf.process(0, kl, sc_mode);
            let dr = self.svf.process(1, kr, sc_mode);

            // Detector level per channel, then linked towards the louder one.
            let (al, ar) = if rms {
                let [c0, c1] = &mut self.chans;
                c0.ms = flush(c0.ms + self.k_rms * (dl * dl - c0.ms));
                c1.ms = flush(c1.ms + self.k_rms * (dr * dr - c1.ms));
                (c0.ms.sqrt(), c1.ms.sqrt())
            } else {
                (dl.abs(), dr.abs())
            };
            let mut el = lin_to_db(al);
            let mut er = lin_to_db(ar);
            let emax = el.max(er);
            el += link * (emax - el);
            er += link * (emax - er);
            m_level = m_level.max(emax);

            let grl = Self::reduce(&mut self.chans[0], el, &cp, &b);
            let grr = Self::reduce(&mut self.chans[1], er, &cp, &b);
            m_gr = m_gr.max(grl).max(grr);

            let yl = self.delays[0].process(xl);
            let yr = self.delays[1].process(xr);
            let ll = self.delays[2].process(dl);
            let lr = self.delays[3].process(dr);
            if listen {
                l[i] = ll;
                r[i] = lr;
                m_out = m_out.max(ll.abs()).max(lr.abs());
                continue;
            }

            let wet_l = Self::colour(&mut self.chans[0], circuit, abi, grl, yl * db_to_lin(-grl));
            let wet_r = Self::colour(&mut self.chans[1], circuit, abi, grr, yr * db_to_lin(-grr));
            let g = db_to_lin(gain_db);
            let wet_l = distortion.apply(wet_l * g);
            let wet_r = distortion.apply(wet_r * g);
            let ol = (yl * (1.0 - mix) + wet_l * mix) * out_gain;
            let or = (yr * (1.0 - mix) + wet_r * mix) * out_gain;
            l[i] = ol;
            r[i] = or;
            m_out = m_out.max(ol.abs()).max(or.abs());
        }
        self.m_in = lin_to_db(m_in);
        self.m_out = lin_to_db(m_out);
        self.m_gr = m_gr;
        self.m_level = m_level;
    }

    fn reset(&mut self) {
        for s in [
            &mut self.threshold,
            &mut self.ratio,
            &mut self.knee,
            &mut self.makeup,
            &mut self.auto_gain,
            &mut self.mix,
            &mut self.output,
            &mut self.link,
        ] {
            s.snap();
        }
        for ch in &mut self.chans {
            let mut dc = ch.dc;
            dc.reset();
            *ch = Chan { dc, ..Chan::default() };
        }
        self.svf.reset();
        for d in &mut self.delays {
            d.reset();
        }
        self.m_in = -120.0;
        self.m_out = -120.0;
        self.m_gr = 0.0;
        self.m_level = -120.0;
    }

    fn latency(&self) -> usize {
        self.lookahead
    }

    fn tail_secs(&self) -> f32 {
        self.lookahead as f32 / self.sr
    }

    fn meters(&self, out: &mut [f32; MAX_METERS]) {
        out[0] = self.m_in;
        out[1] = self.m_out;
        out[2] = self.m_gr;
        out[3] = self.m_level;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48000.0;
    const CTX: FxContext = FxContext { sample_rate: SR, tempo: 120.0, playing: true };

    fn comp(settings: &[(u32, f32)]) -> Compressor {
        let mut c = Compressor::new(SR);
        for &(id, v) in settings {
            c.set_param(id, v);
        }
        c.reset();
        c
    }

    /// Runs a constant (DC) level through and returns (output dB, gain reduction dB) at the
    /// end. DC makes the peak and RMS detectors read the level exactly.
    fn settle_dc(c: &mut Compressor, level_db: f32, secs: f32) -> (f32, f32) {
        let x = db_to_lin(level_db);
        let mut l = [x; 480];
        let mut r = [x; 480];
        let blocks = (secs * SR / 480.0) as usize;
        for _ in 0..blocks {
            l.fill(x);
            r.fill(x);
            c.process(&mut l, &mut r, None, &CTX);
        }
        let mut m = [0.0; MAX_METERS];
        c.meters(&mut m);
        (lin_to_db(l[479].abs()), m[2])
    }

    /// Runs `secs` of a signal and records the gain reduction meter once per 1 ms block.
    fn gr_trace(c: &mut Compressor, level_db: f32, secs: f32) -> Vec<f32> {
        let x = db_to_lin(level_db);
        let mut out = Vec::new();
        let mut l = [x; 48];
        let mut r = [x; 48];
        let mut m = [0.0; MAX_METERS];
        for _ in 0..(secs * 1000.0) as usize {
            l.fill(x);
            r.fill(x);
            c.process(&mut l, &mut r, None, &CTX);
            c.meters(&mut m);
            out.push(m[2]);
        }
        out
    }

    /// Milliseconds until the trace first crosses `level`, going in its direction.
    fn time_to(trace: &[f32], level: f32) -> f32 {
        let rising = trace.last().unwrap() >= &trace[0];
        trace.iter().position(|&g| if rising { g >= level } else { g <= level }).unwrap_or(trace.len()) as f32
    }

    #[test]
    fn platinum_static_curve() {
        let mut c = comp(&[(param::THRESHOLD, -20.0), (param::RATIO, 4.0), (param::KNEE, 0.0), (param::ATTACK, 1.0), (param::RELEASE, 50.0)]);
        for (inp, want) in [(-40.0, -40.0), (-20.0, -20.0), (-10.0, -17.5), (0.0, -15.0)] {
            let (out, _) = settle_dc(&mut c, inp, 0.5);
            assert!((out - want).abs() < 0.05, "in {inp}: out {out}, want {want}");
        }
        // With a 12 dB knee, the threshold itself gets (1 - 1/R)·(K/2)²/(2K) = 1.125 dB.
        let mut c = comp(&[(param::THRESHOLD, -20.0), (param::RATIO, 4.0), (param::KNEE, 12.0), (param::ATTACK, 1.0), (param::RELEASE, 20.0)]);
        let (out, _) = settle_dc(&mut c, -20.0, 0.5);
        assert!((out - -21.125).abs() < 0.05, "knee: {out}");
        let (out, _) = settle_dc(&mut c, -30.0, 0.5);
        assert!((out - -30.0).abs() < 0.01, "below the knee: {out}");
    }

    #[test]
    fn vca_and_fet_static_curves() {
        for detector in [0.0, 1.0] {
            let mut c = comp(&[(param::CIRCUIT, 1.0), (param::THRESHOLD, -30.0), (param::RATIO, 3.0), (param::KNEE, 0.0), (param::DETECTOR, detector)]);
            let (out, _) = settle_dc(&mut c, -6.0, 3.0);
            assert!((out - -22.0).abs() < 0.1, "vca: {out}");
        }
        // The FET's feedback loop lands on the button's ratio well above the knee.
        for (mode, ratio) in [(0.0, 4.0), (1.0, 8.0), (2.0, 12.0), (3.0, 20.0)] {
            let mut c = comp(&[(param::CIRCUIT, 2.0), (param::THRESHOLD, -40.0), (param::FET_RATIO, mode)]);
            let (o1, _) = settle_dc(&mut c, -10.0, 3.0);
            let (o2, _) = settle_dc(&mut c, 0.0, 3.0);
            let measured = 10.0 / (o2 - o1);
            assert!((measured - ratio).abs() / ratio < 0.08, "fet {ratio}:1 measured {measured}");
            let gr = static_reduction(&c.curve_params(), 0.0);
            assert!((o2 - (0.0 - gr)).abs() < 0.3, "fet static {o2} vs {}", -gr);
        }
    }

    #[test]
    fn fet_all_buttons_overcompresses() {
        let mut c = comp(&[(param::CIRCUIT, 2.0), (param::THRESHOLD, -40.0), (param::FET_RATIO, 4.0)]);
        let (o1, _) = settle_dc(&mut c, -20.0, 3.0);
        let (o2, _) = settle_dc(&mut c, -6.0, 3.0);
        assert!(o2 < o1, "all buttons: louder in should be quieter out ({o1} → {o2})");
    }

    /// The local ratio (input dB per output dB) of the steady-state curve at `x`.
    fn local_ratio(p: &CurveParams, x: f32) -> f32 {
        let y = |x: f32| x - static_reduction(p, x);
        1.0 / (y(x + 0.5) - y(x - 0.5))
    }

    #[test]
    fn opto_and_vari_mu_ratio_rises_with_level() {
        for circuit in [Circuit::Opto, Circuit::VariMu] {
            let p = CurveParams { circuit, threshold: -30.0, ratio: 6.0, knee: 0.0, fet_mode: 0, opto_limit: false };
            let ratios: Vec<f32> = [-40.0, -32.0, -26.0, -18.0, -6.0].iter().map(|&x| local_ratio(&p, x)).collect();
            assert!(ratios.windows(2).all(|w| w[1] > w[0]), "{circuit:?}: {ratios:?}");
            assert!(ratios[0] < 1.3 && *ratios.last().unwrap() > 2.0, "{circuit:?}: {ratios:?}");
        }
        // Limit is steeper than Compress.
        let comp_p = CurveParams { circuit: Circuit::Opto, threshold: -30.0, ratio: 1.0, knee: 0.0, fet_mode: 0, opto_limit: false };
        let lim_p = CurveParams { opto_limit: true, ..comp_p };
        assert!(local_ratio(&lim_p, -10.0) > 2.0 * local_ratio(&comp_p, -10.0));
        // And the running compressor settles onto its static curve.
        for circuit in [3.0, 4.0] {
            let mut c = comp(&[(param::CIRCUIT, circuit), (param::THRESHOLD, -30.0), (param::RATIO, 6.0)]);
            let (_, gr) = settle_dc(&mut c, -10.0, 20.0);
            let want = static_reduction(&c.curve_params(), -10.0);
            assert!((gr - want).abs() < 0.1, "circuit {circuit}: gr {gr} want {want}");
        }
    }

    #[test]
    fn platinum_attack_and_release_match_the_knobs() {
        // Threshold -20, 4:1, hard knee: a step to -10 dB settles at 7.5 dB of reduction. A
        // one-pole envelope reaches 63 % of a step in one time constant.
        let mut c = comp(&[(param::THRESHOLD, -20.0), (param::RATIO, 4.0), (param::KNEE, 0.0), (param::ATTACK, 20.0), (param::RELEASE, 200.0)]);
        let up = gr_trace(&mut c, -10.0, 0.4);
        assert!((up.last().unwrap() - 7.5).abs() < 0.01);
        let t_att = time_to(&up, 7.5 * (1.0 - (-1.0f32).exp()));
        assert!((t_att - 20.0).abs() <= 1.5, "attack {t_att} ms");
        let down = gr_trace(&mut c, -40.0, 1.5);
        let t_rel = time_to(&down, 7.5 * (-1.0f32).exp());
        assert!((t_rel - 200.0).abs() <= 3.0, "release {t_rel} ms");
    }

    #[test]
    fn fet_attacks_faster_than_opto() {
        let attack_ms = |circuit: f32| {
            let mut c = comp(&[(param::CIRCUIT, circuit), (param::THRESHOLD, -30.0), (param::ATTACK, 0.02), (param::FET_RATIO, 1.0)]);
            let trace = gr_trace(&mut c, -6.0, 0.3);
            let fin = *trace.last().unwrap();
            time_to(&trace, fin * 0.63)
        };
        let fet = attack_ms(2.0);

        let opto = attack_ms(3.0);
        assert!(fet <= 1.0 && opto >= 5.0, "fet {fet} ms, opto {opto} ms");
    }

    #[test]
    fn opto_release_lengthens_after_long_heavy_compression() {
        let release_ms = |hold_secs: f32| {
            let mut c = comp(&[(param::CIRCUIT, 3.0), (param::THRESHOLD, -40.0)]);
            gr_trace(&mut c, -6.0, hold_secs);
            let trace = gr_trace(&mut c, -80.0, 20.0);
            time_to(&trace, 1.0)
        };
        let short = release_ms(0.1);
        let long = release_ms(8.0);
        assert!(short < 400.0, "short burst released in {short} ms");
        assert!(long > 4.0 * short && long > 2000.0, "long compression released in {long} ms (short {short})");
    }

    #[test]
    fn opto_releases_in_two_stages() {
        let mut c = comp(&[(param::CIRCUIT, 3.0), (param::THRESHOLD, -40.0)]);
        let held = *gr_trace(&mut c, -6.0, 8.0).last().unwrap();
        let trace = gr_trace(&mut c, -80.0, 10.0);
        // Most of the first half goes quickly, the rest hangs on for seconds.
        assert!(trace[200] < 0.6 * held && trace[200] > 0.3 * held, "after 200 ms: {} of {held}", trace[200]);
        assert!(trace[2000] > 0.15 * held, "after 2 s: {} of {held}", trace[2000]);
    }

    #[test]
    fn mix_zero_is_exactly_dry() {
        for circuit in 0..5 {
            let mut c = comp(&[(param::CIRCUIT, circuit as f32), (param::MIX, 0.0), (param::THRESHOLD, -40.0), (param::DISTORTION, 1.0)]);
            let src: Vec<f32> = (0..4800).map(|i| (i as f32 * 0.05).sin() * 0.8).collect();
            let mut l = src.clone();
            let mut r = src.clone();
            for (bl, br) in l.chunks_mut(512).zip(r.chunks_mut(512)) {
                c.process(bl, br, None, &CTX);
            }
            assert_eq!(l, src, "circuit {circuit}");
            assert_eq!(r, src);
        }
    }

    #[test]
    fn auto_gain_is_roughly_level_neutral() {
        for (setting, level) in [(2.0, -12.0), (3.0, -24.0), (1.0, 0.0)] {
            let mut c = comp(&[(param::THRESHOLD, -30.0), (param::RATIO, 4.0), (param::AUTO_GAIN, setting), (param::ATTACK, 1.0)]);
            let (out, gr) = settle_dc(&mut c, level, 0.5);
            assert!(gr > 4.0, "auto gain {setting}: gr {gr} out {out}");
            assert!((out - level).abs() < 0.1, "auto gain {setting}: in {level} out {out}");
        }
    }

    #[test]
    fn lookahead_reports_latency_and_delays_audio() {
        let mut c = comp(&[(param::LOOKAHEAD, 5.0), (param::THRESHOLD, 0.0)]);
        assert_eq!(c.latency(), 240);
        let mut l = vec![0.0f32; 1024];
        let mut r = vec![0.0f32; 1024];
        l[0] = 0.5;
        r[0] = 0.5;
        c.process(&mut l, &mut r, None, &CTX);
        assert_eq!(l[240], 0.5);
        assert!(l.iter().enumerate().all(|(i, &v)| i == 240 || v == 0.0));
    }

    #[test]
    fn side_chain_key_drives_the_detector() {
        let mut c = comp(&[(param::THRESHOLD, -30.0), (param::RATIO, 10.0), (param::KNEE, 0.0), (param::ATTACK, 1.0)]);
        let mut l = [0.1f32; 480];
        let mut r = [0.1f32; 480];
        let key = [0.9f32; 480];
        for _ in 0..50 {
            l.fill(0.1);
            r.fill(0.1);
            c.process(&mut l, &mut r, Some((&key, &key)), &CTX);
        }
        // The quiet input (-20 dB, under the threshold by itself) is ducked by the loud key.
        assert!(l[479] < 0.02, "{}", l[479]);
    }

    #[test]
    fn listen_hears_the_filtered_key() {
        let mut c = comp(&[(param::LISTEN, 1.0), (param::SC_MODE, 1.0), (param::SC_FREQ, 1000.0)]);
        // DC goes through a high-pass side chain as nothing.
        let mut l = [0.5f32; 1024];
        let mut r = [0.5f32; 1024];
        for _ in 0..20 {
            l.fill(0.5);
            r.fill(0.5);
            c.process(&mut l, &mut r, None, &CTX);
        }
        assert!(l[1023].abs() < 1e-3);
    }

    #[test]
    fn silence_stays_clean() {
        for circuit in 0..5 {
            let mut c = comp(&[(param::CIRCUIT, circuit as f32), (param::SC_MODE, 3.0), (param::DETECTOR, 1.0), (param::AUTO_RELEASE, 1.0), (param::TIME_CONSTANT, 5.0)]);
            // A loud burst, then a long silence: every state decays to exact zeros.
            gr_trace(&mut c, -3.0, 0.2);
            let mut l = [0.0f32; 1024];
            let mut r = [0.0f32; 1024];
            for _ in 0..2000 {
                l.fill(0.0);
                r.fill(0.0);
                c.process(&mut l, &mut r, None, &CTX);
            }
            assert!(l.iter().chain(&r).all(|v| v.is_finite() && (*v == 0.0 || v.abs() > f32::MIN_POSITIVE)), "circuit {circuit}");
            for ch in &c.chans {
                for v in [ch.ms, ch.gr, ch.slow, ch.slower, ch.memory] {
                    assert!(v == 0.0 || v.abs() > f32::MIN_POSITIVE, "circuit {circuit}: denormal state {v}");
                }
            }
        }
    }

    #[test]
    fn presets_are_valid() {
        assert!((10..=14).contains(&PRESETS.len()));
        super::super::dyn_common::check_presets(&PARAMS, PRESETS);
    }

    #[test]
    fn vca_snaps_to_console_positions() {
        let c = comp(&[(param::CIRCUIT, 1.0), (param::ATTACK, 2.0), (param::RELEASE, 900.0)]);
        assert_eq!(c.effective_times(), (3.0, 1200.0));
    }
}
