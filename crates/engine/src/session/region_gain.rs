//! Region gain set per region, as a gain drag in the Tracks area sends it: every selected
//! audio region's gain moves by the same number of dB from where it was, so each gets its own
//! value.

use super::*;

/// Region gain's range in dB. The bottom is shown as −∞.
pub const REGION_GAIN_MIN: f32 = -96.0;
pub const REGION_GAIN_MAX: f32 = 30.0;

impl Session {
    /// Sets each listed audio region's gain (dB), clamped to the region gain range.
    pub fn set_region_gains(&mut self, gains: &[(Id, f32)]) {
        let ids: Vec<Id> = gains.iter().map(|g| g.0).collect();
        self.edit_regions(&ids, |r, _| {
            let Some(&(_, db)) = gains.iter().find(|g| g.0 == r.id) else { return };
            if let RegionContent::Audio { gain_db, .. } = &mut r.content {
                *gain_db = if db.is_finite() { db.clamp(REGION_GAIN_MIN, REGION_GAIN_MAX) } else { REGION_GAIN_MIN };
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_io::AudioConfig;

    #[test]
    fn gains_are_set_per_region_and_clamped() {
        let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
        let mut s = Session::new(cfg, Vec::new());
        s.dir = std::env::temp_dir().join(format!("fermata-gain-{}", std::process::id()));
        let path = s.dir.join("Audio Files").join("dc.wav");
        let clip = s.add_clip(&path, Clip::from_channels(path.clone(), 48_000, vec![vec![0.25; 4_800]]));
        let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        let mut ids = Vec::new();
        for k in 0..3 {
            let id = s.project.alloc_id();
            let content = RegionContent::Audio { clip, offset: 0, gain_db: 0.0, fade_in: 0, fade_out: 0, fade_shape: Default::default() };
            s.project.track_mut(t).unwrap().regions.push(Region { id, name: "DC".into(), start: k * 10_000, length: 4_800, muted: false, color: None, content, loop_end: None, takes: None, params: Default::default(), pattern: None });
            ids.push(id);
        }
        s.set_region_gains(&[(ids[0], -6.0), (ids[1], 45.0), (ids[2], f32::NEG_INFINITY)]);
        let gain = |id: Id| match s.region(id).unwrap().content {
            RegionContent::Audio { gain_db, .. } => gain_db,
            _ => unreachable!(),
        };
        assert_eq!((gain(ids[0]), gain(ids[1]), gain(ids[2])), (-6.0, 30.0, -96.0));
    }
}
