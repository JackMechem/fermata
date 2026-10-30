//! VCA faders on the session side: creating, editing and assigning them. The levels reach the
//! engine through `track_params` (see `crate::vca`), so every edit here just resends the
//! tracks' parameters, and undo is covered by `restore()`'s `push_all_params`.

use super::*;
use crate::vca::{can_assign, can_nest, next_name, MAX_DB, MIN_DB};

/// The VCA strip's settings an edit changes (None leaves one alone).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct VcaPatch {
    pub name: Option<String>,
    pub color: Option<String>,
    pub level_db: Option<f32>,
    pub mute: Option<bool>,
}

/// The colours new VCAs take in turn.
const VCA_COLORS: [&str; 4] = ["#c9a227", "#5bc0de", "#b07cd8", "#e0709a"];

impl Session {
    pub fn vca(&self, id: Id) -> Option<&Vca> {
        self.project.vcas.iter().find(|v| v.id == id)
    }

    /// Mixer › Options › Create New VCA: a VCA at 0 dB, with `tracks` assigned to it.
    pub fn create_vca(&mut self, tracks: &[Id]) -> Id {
        let id = self.project.alloc_id();
        let name = next_name(&self.project.vcas);
        let color = VCA_COLORS[self.project.vcas.len() % VCA_COLORS.len()].to_string();
        self.project.vcas.push(Vca { id, name, color, level_db: 0.0, mute: false, parent: None });
        if !tracks.is_empty() {
            // Folders are skipped; the VCA stays either way.
            let _ = self.assign_vca(tracks, Some(id));
        }
        self.dirty = true;
        id
    }

    /// Deletes a VCA. Its strips keep their own faders, and VCAs nested in it move up to its
    /// parent.
    pub fn delete_vca(&mut self, id: Id) {
        let Some(gone) = self.vca(id).cloned() else { return };
        for t in self.project.tracks.iter_mut().filter(|t| t.vca == Some(id)) {
            t.vca = None;
        }
        for v in self.project.vcas.iter_mut().filter(|v| v.parent == Some(id)) {
            v.parent = gone.parent;
        }
        self.project.vcas.retain(|v| v.id != id);
        self.push_vca_params();
        self.dirty = true;
    }

    pub fn update_vca(&mut self, id: Id, patch: VcaPatch) {
        let Some(v) = self.project.vcas.iter_mut().find(|v| v.id == id) else { return };
        if let Some(name) = patch.name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()) {
            v.name = name;
        }
        if let Some(c) = patch.color {
            v.color = c;
        }
        if let Some(db) = patch.level_db {
            v.level_db = db.clamp(MIN_DB, MAX_DB);
        }
        if let Some(m) = patch.mute {
            v.mute = m;
        }
        self.push_vca_params();
        self.dirty = true;
    }

    /// Assigns tracks to a VCA (None: no VCA). Folder stacks can't join one.
    pub fn assign_vca(&mut self, tracks: &[Id], vca: Option<Id>) -> Result<()> {
        if let Some(v) = vca {
            self.vca(v).ok_or_else(|| anyhow!("no such VCA"))?;
        }
        let mut any = false;
        for t in self.project.tracks.iter_mut().filter(|t| tracks.contains(&t.id) && can_assign(t.kind)) {
            t.vca = vca;
            any = true;
        }
        if !any {
            return Err(anyhow!("a folder stack has no audio of its own for a VCA to control"));
        }
        self.push_vca_params();
        self.dirty = true;
        Ok(())
    }

    /// Assigns a VCA to another VCA (None: to none), refusing loops.
    pub fn set_vca_parent(&mut self, id: Id, parent: Option<Id>) -> Result<()> {
        let name = self.vca(id).ok_or_else(|| anyhow!("no such VCA"))?.name.clone();
        if let Some(p) = parent {
            let up = self.vca(p).ok_or_else(|| anyhow!("no such VCA"))?;
            if !can_nest(&self.project.vcas, id, p) {
                return Err(anyhow!("{} is already controlled by {name}, so this would make a loop", up.name));
            }
        }
        self.project.vcas.iter_mut().find(|v| v.id == id).unwrap().parent = parent;
        self.push_vca_params();
        self.dirty = true;
        Ok(())
    }

    /// Resends every track's parameters. Unlike `push_all_params` it leaves delay compensation
    /// alone, since a VCA only changes gains; this runs on every step of a VCA fader drag.
    fn push_vca_params(&mut self) {
        let params: Vec<(Id, TrackParams)> = self.project.tracks.iter().map(|t| (t.id, Self::track_params(&self.project, t))).collect();
        for (id, p) in params {
            self.send(Cmd::Params(id, p));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_io::AudioConfig;

    /// Two audio tracks, the second in a folder stack.
    fn session() -> (Session, Id, Id, Id) {
        let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
        let mut s = Session::new(cfg, Vec::new());
        let a = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        let b = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        let f = s.create_stack(&[b], false).unwrap();
        (s, a, b, f)
    }

    #[test]
    fn a_vca_offsets_its_members_and_mutes_them() {
        let (mut s, a, b, folder) = session();
        s.update_track(a, TrackPatch { volume_db: Some(-2.0), ..Default::default() });
        let v = s.create_vca(&[a, folder]);
        assert_eq!(s.project.track(a).unwrap().vca, Some(v));
        assert_eq!(s.project.track(folder).unwrap().vca, None, "folders don't join");
        s.update_vca(v, VcaPatch { level_db: Some(-6.0), ..Default::default() });
        let p = Session::track_params(&s.project, s.project.track(a).unwrap());
        assert!((p.volume - crate::dsp::volume_gain(-8.0)).abs() < 1e-6);
        assert!((p.stack_gain - crate::dsp::volume_gain(-6.0)).abs() < 1e-6, "automation keeps the VCA offset");
        assert_eq!(s.project.track(a).unwrap().volume_db, -2.0, "the member's fader doesn't move");
        let other = Session::track_params(&s.project, s.project.track(b).unwrap());
        assert_eq!(other.volume, 1.0);

        s.update_vca(v, VcaPatch { mute: Some(true), ..Default::default() });
        let p = Session::track_params(&s.project, s.project.track(a).unwrap());
        assert!(p.mute && p.stack_mute && !s.project.track(a).unwrap().mute);
    }

    #[test]
    fn nesting_refuses_loops_and_deleting_lifts_children() {
        let (mut s, a, _, _) = session();
        let top = s.create_vca(&[]);
        let mid = s.create_vca(&[]);
        let low = s.create_vca(&[a]);
        s.set_vca_parent(low, Some(mid)).unwrap();
        s.set_vca_parent(mid, Some(top)).unwrap();
        assert!(s.set_vca_parent(top, Some(low)).is_err());
        s.update_vca(top, VcaPatch { level_db: Some(-10.0), ..Default::default() });
        let p = Session::track_params(&s.project, s.project.track(a).unwrap());
        assert!((p.volume - crate::dsp::volume_gain(-10.0)).abs() < 1e-6, "a parent's level reaches nested members");
        s.delete_vca(mid);
        assert_eq!(s.vca(low).unwrap().parent, Some(top));
        s.delete_vca(low);
        assert_eq!(s.project.track(a).unwrap().vca, None);
        let p = Session::track_params(&s.project, s.project.track(a).unwrap());
        assert_eq!(p.volume, 1.0);
    }

    #[test]
    fn levels_are_clamped_and_names_kept() {
        let (mut s, ..) = session();
        let v = s.create_vca(&[]);
        s.update_vca(v, VcaPatch { level_db: Some(20.0), name: Some("  ".into()), ..Default::default() });
        assert_eq!(s.vca(v).unwrap().level_db, 6.0);
        assert_eq!(s.vca(v).unwrap().name, "VCA 1");
        s.update_vca(v, VcaPatch { name: Some("Drums".into()), ..Default::default() });
        assert_eq!(s.vca(v).unwrap().name, "Drums");
        let w = s.create_vca(&[]);
        assert_eq!(s.vca(w).unwrap().name, "VCA 1");
    }
}
