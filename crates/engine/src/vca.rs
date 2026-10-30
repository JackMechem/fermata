//! VCA faders: how a VCA's level and mute reach the strips assigned to it.
//!
//! A VCA passes no audio. The engine never sees one: `Session::track_params` adds the VCA
//! offset to a member's volume and to its `stack_gain`, the part of the volume that still
//! applies when automation drives the track's own fader (see `dsp::TrackParams`). So the
//! audio thread's fader smoothing covers VCA moves, and automated volume combines with them.

use crate::model::{Id, TrackKind, Vca};

/// The lowest VCA level; at or below it the VCA silences its members.
pub const MIN_DB: f32 = -96.0;
/// The highest VCA level, as for a channel fader.
pub const MAX_DB: f32 = 6.0;

/// Logic lets audio, instrument and aux strips (and buses) join a VCA. A folder stack has no
/// audio of its own, so it has none.
pub fn can_assign(kind: TrackKind) -> bool {
    !matches!(kind, TrackKind::Folder)
}

/// The VCAs a strip on `first` answers to: `first`, then the VCA it is assigned to, and so on.
/// A missing VCA ends the chain, and so does a loop (which the edits refuse, but a hand-edited
/// file could hold).
pub fn chain(vcas: &[Vca], first: Option<Id>) -> Vec<&Vca> {
    let mut out: Vec<&Vca> = Vec::new();
    let mut next = first;
    while let Some(id) = next {
        let Some(v) = vcas.iter().find(|v| v.id == id) else { break };
        if out.iter().any(|o| o.id == id) {
            break;
        }
        out.push(v);
        next = v.parent;
    }
    out
}

/// The offset (dB) and mute a strip assigned to `vca` gets: the levels along the chain add up,
/// and any muted VCA mutes it. A VCA at the bottom of its travel silences its members whatever
/// their faders say, so the offset is then minus infinity.
pub fn offset(vcas: &[Vca], vca: Option<Id>) -> (f32, bool) {
    let mut db = 0.0f32;
    let mut mute = false;
    for v in chain(vcas, vca) {
        if v.level_db <= MIN_DB {
            db = f32::NEG_INFINITY;
        } else {
            db += v.level_db.min(MAX_DB);
        }
        mute |= v.mute;
    }
    (db, mute)
}

/// Whether `vca` may be assigned to `parent` without making a loop.
pub fn can_nest(vcas: &[Vca], vca: Id, parent: Id) -> bool {
    vca != parent && !chain(vcas, Some(parent)).iter().any(|v| v.id == vca)
}

/// The next free "VCA n" name.
pub fn next_name(vcas: &[Vca]) -> String {
    let n = (1..).find(|n| !vcas.iter().any(|v| v.name == format!("VCA {n}"))).unwrap();
    format!("VCA {n}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vca(id: Id, level_db: f32, mute: bool, parent: Option<Id>) -> Vca {
        Vca { id, name: format!("VCA {id}"), color: String::new(), level_db, mute, parent }
    }

    #[test]
    fn levels_add_up_along_the_chain() {
        let vcas = [vca(1, -6.0, false, None), vca(2, -3.0, false, Some(1)), vca(3, 2.0, false, Some(2))];
        assert_eq!(offset(&vcas, None), (0.0, false));
        assert_eq!(offset(&vcas, Some(1)), (-6.0, false));
        assert_eq!(offset(&vcas, Some(2)), (-9.0, false));
        assert_eq!(offset(&vcas, Some(3)), (-7.0, false));
        // A VCA that no longer exists offsets nothing.
        assert_eq!(offset(&vcas, Some(99)), (0.0, false));
    }

    #[test]
    fn a_muted_vca_mutes_everything_below_it() {
        let vcas = [vca(1, 0.0, true, None), vca(2, 0.0, false, Some(1)), vca(3, 0.0, false, None)];
        assert!(offset(&vcas, Some(1)).1);
        assert!(offset(&vcas, Some(2)).1, "the parent's mute reaches a nested VCA's strips");
        assert!(!offset(&vcas, Some(3)).1);
    }

    #[test]
    fn a_vca_at_the_bottom_silences_its_members() {
        let vcas = [vca(1, MIN_DB, false, None), vca(2, 6.0, false, Some(1))];
        let (db, _) = offset(&vcas, Some(2));
        assert_eq!(db, f32::NEG_INFINITY);
        // However far up the member's own fader is.
        assert_eq!(crate::dsp::volume_gain((6.0 + db).max(-96.0)), 0.0);
    }

    #[test]
    fn loops_are_refused_and_survived() {
        let mut vcas = vec![vca(1, -1.0, false, None), vca(2, -1.0, false, Some(1)), vca(3, -1.0, false, Some(2))];
        assert!(can_nest(&vcas, 3, 1));
        assert!(!can_nest(&vcas, 1, 3), "1 is already above 3");
        assert!(!can_nest(&vcas, 2, 2));
        // A loop in a loaded file is cut where it closes.
        vcas[0].parent = Some(3);
        assert_eq!(chain(&vcas, Some(1)).len(), 3);
        assert_eq!(offset(&vcas, Some(2)).0, -3.0);
    }

    #[test]
    fn names_count_up() {
        assert_eq!(next_name(&[]), "VCA 1");
        assert_eq!(next_name(&[vca(5, 0.0, false, None)]), "VCA 1");
        let mut v = vca(5, 0.0, false, None);
        v.name = "VCA 1".into();
        assert_eq!(next_name(&[v]), "VCA 2");
    }

    #[test]
    fn old_projects_have_no_vcas() {
        let json = r##"{"id":1,"name":"VCA 1","color":"#fff","level_db":-3.0,"mute":false}"##;
        let v: Vca = serde_json::from_str(json).unwrap();
        assert_eq!(v.parent, None);
    }
}
