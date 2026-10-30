//! The Marker track and the Autopunch range. Markers are only project data (the UI draws them
//! and locates to them), so editing them never touches the engine.

use super::*;

impl Session {
    fn sort_markers(&mut self) {
        self.project.markers.sort_by_key(|m| (m.pos, m.id));
        self.dirty = true;
    }

    /// Create Marker: a marker at `pos`, named `name` or "Marker n". Returns its id (the
    /// existing one if a marker already sits there).
    pub fn add_marker(&mut self, pos: i64, name: Option<String>) -> Id {
        let pos = pos.max(0);
        if let Some(m) = self.project.markers.iter().find(|m| m.pos == pos) {
            return m.id;
        }
        let name = name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| {
            let taken = |n: &str| self.project.markers.iter().any(|m| m.name == n);
            (1..).map(|k| format!("Marker {k}")).find(|n| !taken(n)).unwrap()
        });
        let id = self.project.alloc_id();
        self.project.markers.push(Marker { id, pos, name, color: None, length: None });
        self.sort_markers();
        id
    }

    pub fn delete_markers(&mut self, ids: &[Id]) {
        self.project.markers.retain(|m| !ids.contains(&m.id));
        self.dirty = true;
    }

    pub fn rename_marker(&mut self, id: Id, name: String) {
        if let Some(m) = self.project.markers.iter_mut().find(|m| m.id == id) {
            m.name = name;
            self.dirty = true;
        }
    }

    pub fn move_marker(&mut self, id: Id, pos: i64) {
        if let Some(m) = self.project.markers.iter_mut().find(|m| m.id == id) {
            m.pos = pos.max(0);
            self.sort_markers();
        }
    }

    /// None returns the marker to the default colour.
    pub fn set_marker_color(&mut self, id: Id, color: Option<String>) {
        if let Some(m) = self.project.markers.iter_mut().find(|m| m.id == id) {
            m.color = color;
            self.dirty = true;
        }
    }

    /// Gives a marker its own length (Logic's marker length), or None (or a length under one
    /// sample) to make it reach the next marker again.
    pub fn set_marker_length(&mut self, id: Id, length: Option<i64>) {
        if let Some(m) = self.project.markers.iter_mut().find(|m| m.id == id) {
            m.length = length.filter(|&l| l > 0);
            self.dirty = true;
        }
    }

    /// Sets the Autopunch range and switches it on or off. Like the cycle, it isn't an undo
    /// step.
    pub fn set_punch(&mut self, enabled: bool, start: i64, end: i64) {
        let (start, end) = (start.min(end).max(0), start.max(end).max(0));
        self.project.punch = Punch { enabled: enabled && end > start, start, end };
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_io::AudioConfig;

    #[test]
    fn a_marker_length_is_set_and_cleared() {
        let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
        let mut s = Session::new(cfg, Vec::new());
        let id = s.add_marker(1000, None);
        s.set_marker_length(id, Some(500));
        assert_eq!(s.project.markers[0].length, Some(500));
        // Moving keeps the length.
        s.move_marker(id, 2000);
        assert_eq!((s.project.markers[0].pos, s.project.markers[0].length), (2000, Some(500)));
        s.set_marker_length(id, Some(0));
        assert_eq!(s.project.markers[0].length, None);
        s.set_marker_length(id, Some(10));
        s.set_marker_length(id, None);
        assert_eq!(s.project.markers[0].length, None);
        // Old projects (no length) still load.
        let m: Marker = serde_json::from_str(r#"{"id":1,"pos":5,"name":"A"}"#).unwrap();
        assert_eq!(m.length, None);
        let a: ArrangementMarker = serde_json::from_str(r#"{"id":1,"name":"Intro","start":0,"length":5}"#).unwrap();
        assert_eq!(a.color, None);
    }
}
