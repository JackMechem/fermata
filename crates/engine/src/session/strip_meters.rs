//! What the channel strips show besides the level meter: the clip indicators (the peak readout
//! over each meter) and the gain-reduction meter of a strip with a dynamics effect.
//!
//! - **Peak holds.** Each track's `Meter` reports the peak since the previous tick, so a tick
//!   never misses one. `tick` folds those into [`PeakHolds`], which keeps the highest peak per
//!   strip until the user resets it, as Logic does (playing from the start doesn't reset it).
//!   Holding them here rather than in the UI means every window (the Mixer, the Inspector, a
//!   detached Mixer) shows the same value, and closing the Mixer doesn't forget a clip. The
//!   indicator lights red when the hold is over full scale (`src/stripMeters.ts`).
//! - **Gain reduction.** A built-in dynamics effect keeps its largest gain reduction since the
//!   last tick (`FxShared::take_gain_reduction`), whether or not its panel is open. The strip
//!   shows its first active dynamics effect's.

use super::*;

/// The highest peak (linear gain) each strip reached since its clip indicator was reset.
#[derive(Debug, Default)]
pub struct PeakHolds {
    held: HashMap<Id, f32>,
}

impl PeakHolds {
    /// Takes in a strip's latest peak and returns what it now holds.
    pub fn feed(&mut self, id: Id, peak: f32) -> f32 {
        let h = self.held.entry(id).or_insert(0.0);
        if peak.is_finite() && peak > *h {
            *h = peak;
        }
        *h
    }

    pub fn get(&self, id: Id) -> f32 {
        self.held.get(&id).copied().unwrap_or(0.0)
    }

    /// Resets the given strips, or every strip.
    pub fn reset(&mut self, ids: Option<&[Id]>) {
        match ids {
            Some(ids) => ids.iter().for_each(|id| {
                self.held.remove(id);
            }),
            None => self.held.clear(),
        }
    }

    /// Forgets strips that no longer exist.
    pub fn retain(&mut self, keep: impl Fn(Id) -> bool) {
        self.held.retain(|&id, _| keep(id));
    }

    /// The strips that have played anything, as (id, peak).
    pub fn list(&self) -> Vec<(Id, f32)> {
        let mut v: Vec<(Id, f32)> = self.held.iter().filter(|h| *h.1 > 0.0).map(|(&id, &p)| (id, p)).collect();
        v.sort_unstable_by_key(|h| h.0);
        v
    }
}

impl Session {
    /// Resets the clip indicators of these strips (MASTER_ID for Stereo Out), or of all.
    pub fn reset_peak_holds(&mut self, ids: Option<&[Id]>) {
        self.peak_holds.reset(ids);
    }

    /// A strip's held peak (linear gain): the highest level since its clip indicator was reset.
    pub fn peak_hold(&self, id: Id) -> f32 {
        self.peak_holds.get(id)
    }

    /// Folds the peaks of one tick into the holds and lists them.
    pub(super) fn hold_peaks(&mut self, meters: &[(Id, f32, f32)], master: (f32, f32)) -> Vec<(Id, f32)> {
        for &(id, l, r) in meters {
            self.peak_holds.feed(id, l.max(r));
        }
        self.peak_holds.feed(MASTER_ID, master.0.max(master.1));
        let live = &self.meters;
        self.peak_holds.retain(|id| id == MASTER_ID || live.contains_key(&id));
        self.peak_holds.list()
    }

    /// Each strip's gain reduction (positive dB) since the last tick, for strips with a
    /// built-in dynamics effect: the first one that isn't bypassed, else the first one (which
    /// then reads 0). Every dynamics instance is read, so none piles up a stale maximum.
    pub(super) fn gain_reductions(&self) -> Vec<(Id, f32)> {
        let p = &self.project;
        let tracks = p.tracks.iter().filter(|t| t.frozen.is_none()).map(|t| (t.id, &t.inserts));
        let mut out = Vec::new();
        for (id, inserts) in tracks.chain(std::iter::once((MASTER_ID, &p.master.inserts))) {
            let mut shown: Option<(bool, f32)> = None;
            for s in inserts.iter().flatten() {
                let Some(gr) = self.fx.get(&s.uid).and_then(|f| f.take_gain_reduction()) else { continue };
                let active = !s.bypass;
                match shown {
                    None => shown = Some((active, gr)),
                    Some((false, _)) if active => shown = Some((true, gr)),
                    _ => {}
                }
            }
            if let Some((_, gr)) = shown {
                out.push((id, gr));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holds_keep_the_highest_peak_until_reset() {
        let mut h = PeakHolds::default();
        assert_eq!(h.feed(1, 0.5), 0.5);
        assert_eq!(h.feed(1, 0.25), 0.5, "a lower peak doesn't lower the hold");
        assert_eq!(h.feed(1, 1.4), 1.4);
        assert_eq!(h.feed(1, f32::NAN), 1.4, "a NaN is ignored");
        assert_eq!(h.feed(1, f32::INFINITY), 1.4);
        h.feed(2, 0.1);
        h.feed(3, 0.0);
        assert_eq!(h.list(), vec![(1, 1.4), (2, 0.1)], "silent strips aren't listed");

        h.reset(Some(&[1]));
        assert_eq!((h.get(1), h.get(2)), (0.0, 0.1), "resetting one strip leaves the others");
        assert_eq!(h.feed(1, 0.3), 0.3, "after a reset the hold starts again");
        h.reset(None);
        assert!(h.list().is_empty());

        h.feed(4, 0.2);
        h.feed(5, 0.2);
        h.retain(|id| id != 4);
        assert_eq!(h.list(), vec![(5, 0.2)]);
    }

    #[test]
    fn ticks_hold_the_peaks_between_them() {
        let cfg = crate::audio_io::AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
        let mut s = Session::new(cfg, Vec::new());
        s.dir = std::env::temp_dir().join(format!("fermata-strip-meters-{}", std::process::id()));
        let a = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        // One tick reports a clip, the next a quiet block; the hold remembers the clip.
        let held = s.hold_peaks(&[(a, 0.2, 1.6)], (0.5, 0.4));
        assert_eq!(held, vec![(MASTER_ID, 0.5), (a, 1.6)]);
        let held = s.hold_peaks(&[(a, 0.1, 0.1)], (0.0, 0.0));
        assert_eq!(held, vec![(MASTER_ID, 0.5), (a, 1.6)]);
        s.reset_peak_holds(Some(&[a]));
        assert_eq!(s.peak_hold(a), 0.0);
        assert_eq!(s.peak_hold(MASTER_ID), 0.5);
        // A track that isn't in the session any more is forgotten.
        let held = s.hold_peaks(&[(a, 0.1, 0.1), (999, 0.9, 0.9)], (0.0, 0.0));
        assert_eq!(held, vec![(MASTER_ID, 0.5), (a, 0.1)]);
    }
}
