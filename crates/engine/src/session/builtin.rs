//! Built-in effects (`crate::fx`) in insert slots, on the session side.
//!
//! - A slot's parameter values live in the project (`PluginSlot::params`, plain units), so
//!   saving, undo, duplicating, freezing and copying a slot carry them like any other setting.
//!   Edits go to the running instance as `Cmd::FxParam`.
//! - Automation lanes drive them like VST3 parameters (normalized 0..1 values, which the
//!   engine converts with the parameter's spec), and moving a control while writing records a
//!   pass, as in `auto_write.rs`.
//! - Latency (lookahead) is reported to delay compensation, re-read whenever a parameter
//!   changes and polled from `idle`, since the instance reports it from the audio thread.
//! - Side-chain: a slot can be keyed by another track's post-fader output. The key source
//!   plays before the receiver (`Project::process_order` treats it like a send), loops are
//!   refused, and offline renders of a track bring its key sources along (`key_sources`).
//!   The key is taken where the source leaves its plug-ins, before its compensation delay,
//!   so it lines up with the receiver when the source's own plug-ins add no latency.
//! - Meters and live values reach the UI in the tick, only for the slots whose panel is open
//!   (`watch_builtin`).

use std::collections::HashSet;

use vst3_host::ParamInfo;

use super::*;
use crate::fx::{self, EffectInfo, ParamKind, Unit};

/// A registry entry as the UI sees it (`FxInfo` in src/fx.ts).
#[derive(Debug, Clone, Serialize)]
pub struct BuiltinInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub category: &'static str,
    pub params: Vec<BuiltinParam>,
    pub meters: Vec<&'static str>,
    pub sidechain: bool,
}

/// `FxParamSpec` in src/fx.ts.
#[derive(Debug, Clone, Serialize)]
pub struct BuiltinParam {
    pub id: u32,
    pub name: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub unit: &'static str,
    pub kind: BuiltinKind,
    pub log: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BuiltinKind {
    Continuous,
    Toggle,
    Choice { labels: Vec<&'static str> },
}

/// One open panel's meters and the values its effect runs with (automation included).
#[derive(Debug, Clone, Serialize)]
pub struct FxTick {
    pub uid: Id,
    pub meters: Vec<f32>,
    pub values: Vec<(u32, f32)>,
}

fn unit_name(u: Unit) -> &'static str {
    match u {
        Unit::None => "none",
        Unit::Db => "db",
        Unit::Ms => "ms",
        Unit::Hz => "hz",
        Unit::Ratio => "ratio",
        Unit::Percent => "percent",
        Unit::Semitones => "semitones",
        Unit::Note => "note",
        Unit::Degrees => "deg",
    }
}

fn describe(e: &EffectInfo) -> BuiltinInfo {
    let params = e
        .params
        .iter()
        .map(|p| BuiltinParam {
            id: p.id,
            name: p.name,
            min: p.min,
            max: p.max,
            default: p.default,
            unit: unit_name(p.unit),
            kind: match p.kind {
                ParamKind::Continuous => BuiltinKind::Continuous,
                ParamKind::Toggle => BuiltinKind::Toggle,
                ParamKind::Choice(labels) => BuiltinKind::Choice { labels: labels.to_vec() },
            },
            log: p.log,
        })
        .collect();
    BuiltinInfo { id: e.id, name: e.name, category: e.category, params, meters: e.meters.to_vec(), sidechain: e.sidechain }
}

/// Every built-in effect, for the slot menus, the Library and the panels.
pub fn builtin_effects() -> Vec<BuiltinInfo> {
    fx::registry().iter().map(describe).collect()
}

/// A slot's value for a parameter: saved, or the default.
fn value_of(slot: &PluginSlot, spec: &fx::ParamSpec) -> f32 {
    slot.params.iter().find(|p| p.0 == spec.id).map(|p| p.1).unwrap_or(spec.default)
}

fn store_value(slot: &mut PluginSlot, id: u32, v: f32) {
    match slot.params.iter_mut().find(|p| p.0 == id) {
        Some(p) => p.1 = v,
        None => slot.params.push((id, v)),
    }
}

impl Session {
    /// A built-in effect instance for a slot, with its saved values and side-chain.
    pub(super) fn instantiate_builtin(&mut self, info: &EffectInfo, slot: &PluginSlot) -> Plugin {
        let b = BuiltinDsp::new(info, self.sample_rate() as f32, &slot.params, slot.sidechain);
        let shared = b.shared();
        shared.set_watched(self.fx_watch.contains(&slot.uid));
        self.fx.insert(slot.uid, shared);
        Plugin::Builtin(Box::new(b))
    }

    /// The track (or MASTER_ID), slot index and slot of a built-in effect instance.
    fn find_builtin(&self, uid: Id) -> Option<(Id, usize, &PluginSlot)> {
        let p = &self.project;
        let tracks = p.tracks.iter().map(|t| (t.id, &t.inserts));
        tracks.chain(std::iter::once((MASTER_ID, &p.master.inserts))).find_map(|(id, ins)| {
            ins.iter().enumerate().find_map(|(k, s)| s.as_ref().filter(|s| s.uid == uid && fx::find(&s.plugin_id).is_some()).map(|s| (id, k, s)))
        })
    }

    /// Sets a built-in effect's parameter (plain units, clamped to its range). While the
    /// transport plays in Touch, Latch or Write, the move is recorded as automation.
    pub fn set_builtin_param(&mut self, track: Id, slot: usize, id: u32, value: f32) -> Result<()> {
        self.check_unfrozen(&[track])?;
        let s = self.slot_ref(track, slot).and_then(|s| s.as_mut()).ok_or_else(|| anyhow!("no plug-in in that slot"))?;
        let info = fx::find(&s.plugin_id).ok_or_else(|| anyhow!("{} isn't a built-in effect", s.name))?;
        let spec = info.param(id).ok_or_else(|| anyhow!("{} has no parameter {id}", info.name))?;
        let before = to_normalized(spec, value_of(s, spec));
        let v = crate::dsp::builtin_clamp(spec, value);
        store_value(s, id, v);
        let uid = s.uid;
        self.send(Cmd::FxParam { track, uid, id, value: v });
        // A lookahead may have changed. Without an audio device the engine has already
        // applied the change; otherwise `idle` notices it.
        if self.builtin_latency_changed() {
            self.update_pdc();
        }
        self.dirty = true;
        if track != MASTER_ID {
            self.auto_plugin_moved(track, uid, id, before);
        }
        Ok(())
    }

    /// Every parameter's value in an instance (plain units, defaults filled in), in the
    /// registry's order.
    pub fn builtin_params(&self, uid: Id) -> Vec<(u32, f32)> {
        let Some((_, _, s)) = self.find_builtin(uid) else { return Vec::new() };
        let Some(info) = fx::find(&s.plugin_id) else { return Vec::new() };
        info.params.iter().map(|p| (p.id, value_of(s, p))).collect()
    }

    /// A parameter's value normalized to 0..1, as automation lanes hold it.
    pub(super) fn builtin_normalized(&self, uid: Id, id: u32) -> Option<f32> {
        let (_, _, s) = self.find_builtin(uid)?;
        let info = fx::find(&s.plugin_id)?;
        let spec = info.param(id)?;
        Some(to_normalized(spec, value_of(s, spec)))
    }

    /// Moves a parameter to a normalized value (automation picking a control up), without
    /// recording it.
    pub(super) fn set_builtin_normalized(&mut self, uid: Id, id: u32, norm: f32) {
        let Some((track, slot, s)) = self.find_builtin(uid) else { return };
        let Some(spec) = fx::find(&s.plugin_id).and_then(|i| i.param(id).copied()) else { return };
        let v = to_plain(&spec, norm);
        if let Some(Some(s)) = self.slot_ref(track, slot) {
            store_value(s, id, v);
        }
        self.send(Cmd::FxParam { track, uid, id, value: v });
        self.dirty = true;
    }

    /// An instance's parameters for the automation lane picker (None if `uid` isn't one).
    pub(super) fn builtin_param_infos(&self, uid: Id) -> Option<Vec<ParamInfo>> {
        let (_, _, s) = self.find_builtin(uid)?;
        let info = fx::find(&s.plugin_id)?;
        let infos = info
            .params
            .iter()
            .map(|p| {
                let (steps, units) = match p.kind {
                    ParamKind::Toggle => (1, ""),
                    ParamKind::Choice(labels) => (labels.len().saturating_sub(1) as i32, ""),
                    ParamKind::Continuous => (0, unit_label(p.unit)),
                };
                ParamInfo { id: p.id, name: p.name.to_string(), units: units.to_string(), default: to_normalized(p, p.default) as f64, steps }
            })
            .collect();
        Some(infos)
    }

    pub(super) fn builtin_param_name(&self, uid: Id, id: u32) -> Option<String> {
        let (_, _, s) = self.find_builtin(uid)?;
        fx::find(&s.plugin_id)?.param(id).map(|p| p.name.to_string())
    }

    /// Keys a built-in effect's side-chain from `source`'s post-fader output, or (None) back
    /// to its own input. Refuses sources that would make a feedback loop.
    pub fn set_sidechain(&mut self, track: Id, slot: usize, source: Option<Id>) -> Result<()> {
        self.check_unfrozen(&[track])?;
        if let Some(src) = source {
            let t = self.project.track(src).ok_or_else(|| anyhow!("no such track"))?;
            if src == track {
                return Err(anyhow!("{} can't key its own side-chain", t.name));
            }
            if t.kind == TrackKind::Folder {
                return Err(anyhow!("a folder stack has no audio of its own to key a side-chain"));
            }
            if track != MASTER_ID && self.project.depends(track, src) {
                let name = self.project.track(track).map(|t| t.name.clone()).unwrap_or_default();
                return Err(anyhow!("{name} already feeds {}, so this would make a feedback loop", t.name));
            }
        }
        let s = self.slot_ref(track, slot).and_then(|s| s.as_mut()).ok_or_else(|| anyhow!("no plug-in in that slot"))?;
        let info = fx::find(&s.plugin_id).ok_or_else(|| anyhow!("{} isn't a built-in effect", s.name))?;
        if !info.sidechain && source.is_some() {
            return Err(anyhow!("{} has no side-chain input", info.name));
        }
        s.sidechain = source;
        let uid = s.uid;
        self.send(Cmd::Sidechain { track, uid, source });
        // The key source must now play before this track.
        self.send(Cmd::Order(self.project.process_order()));
        self.update_pdc();
        self.dirty = true;
        Ok(())
    }

    /// Starts or stops sending an instance's meters and live values with each tick (while its
    /// panel is open).
    pub fn watch_builtin(&mut self, uid: Id, on: bool) {
        if on {
            self.fx_watch.insert(uid);
        } else {
            self.fx_watch.remove(&uid);
        }
        if let Some(s) = self.fx.get(&uid) {
            s.set_watched(on);
        }
    }

    pub(super) fn fx_ticks(&self) -> Vec<FxTick> {
        self.fx_watch.iter().filter_map(|uid| self.fx.get(uid).map(|s| FxTick { uid: *uid, meters: s.meters(), values: s.params() })).collect()
    }

    /// A loaded built-in instance's latency (frames).
    pub(super) fn builtin_latency(&self, uid: Id) -> Option<u32> {
        self.fx.get(&uid).map(|s| s.latency())
    }

    /// Whether a loaded built-in effect's latency differs from what delay compensation was
    /// last worked out with.
    pub(super) fn builtin_latency_changed(&self) -> bool {
        let used = |uid: Id| self.pdc_report.strips.iter().flat_map(|s| s.slots.iter()).find(|s| s.0 == uid).map(|s| s.1).unwrap_or(0);
        let p = &self.project;
        let tracks = p.tracks.iter().filter(|t| t.frozen.is_none()).flat_map(|t| t.inserts.iter());
        tracks.chain(p.master.inserts.iter()).flatten().any(|s| self.fx.get(&s.uid).map(|f| f.latency() != used(s.uid)).unwrap_or(false))
    }

    /// The longest tail of a track's built-in effects (seconds).
    pub(super) fn builtin_tail_secs(&self, track: Id) -> f64 {
        let Some(t) = self.project.track(track) else { return 0.0 };
        t.inserts.iter().flatten().filter_map(|s| self.fx.get(&s.uid)).map(|f| f.tail_secs() as f64).fold(0.0, f64::max)
    }

    /// Undo and redo for an instance that stays loaded: sends the values and side-chain that
    /// differ between `had` and `want`.
    pub(super) fn restore_builtin(&mut self, track: Id, had: &PluginSlot, want: &PluginSlot) {
        let Some(info) = fx::find(&want.plugin_id) else { return };
        if had.params != want.params {
            for p in info.params {
                let v = value_of(want, p);
                if value_of(had, p) != v {
                    self.send(Cmd::FxParam { track, uid: want.uid, id: p.id, value: v });
                }
            }
        }
        if had.sidechain != want.sidechain {
            self.send(Cmd::Sidechain { track, uid: want.uid, source: want.sidechain });
        }
    }

    /// After a track's automation changed: its built-in effects' parameters that no lane
    /// drives any more go back to their own values (the engine holds the last automated
    /// value otherwise, which the project wouldn't know about).
    pub(super) fn resync_builtins(&mut self, track: Id) {
        let Some(t) = self.project.track(track) else { return };
        let driven = |uid: Id, id: u32| t.automation.iter().any(|l| !l.points.is_empty() && matches!(l.param, AutoParam::Plugin { uid: u, param, .. } if u == uid && param == id));
        let mut cmds = Vec::new();
        for s in t.inserts.iter().flatten() {
            let Some(info) = fx::find(&s.plugin_id) else { continue };
            for p in info.params.iter().filter(|p| !driven(s.uid, p.id)) {
                cmds.push(Cmd::FxParam { track, uid: s.uid, id: p.id, value: value_of(s, p) });
            }
        }
        for c in cmds {
            self.send(c);
        }
    }

    /// The tracks outside `members` an offline render of them also has to play, so their
    /// side-chains hear the right key: every key source, and whatever feeds those.
    pub(super) fn key_sources(&self, members: &[Id]) -> Vec<Id> {
        let p = &self.project;
        let keys_of = |id: Id| p.track(id).map(|t| t.inserts.iter().flatten().filter_map(|s| s.sidechain).collect::<Vec<_>>()).unwrap_or_default();
        let mut todo: Vec<Id> = members.iter().flat_map(|&m| keys_of(m)).collect();
        let mut out: Vec<Id> = Vec::new();
        let mut seen: HashSet<Id> = members.iter().copied().collect();
        while let Some(id) = todo.pop() {
            if !seen.insert(id) || p.track(id).is_none() {
                continue;
            }
            out.push(id);
            todo.extend(keys_of(id));
            // Its inputs: subtracks of a stack, and tracks routed or sending to it.
            todo.extend(p.tracks.iter().filter(|t| t.parent == Some(id) || p.feeds(t.id).contains(&id)).map(|t| t.id));
        }
        out
    }
}

/// A unit's suffix for the automation lane's readout.
fn unit_label(u: Unit) -> &'static str {
    match u {
        Unit::Db => "dB",
        Unit::Ms => "ms",
        Unit::Hz => "Hz",
        Unit::Percent => "%",
        Unit::Semitones => "st",
        Unit::Degrees => "°",
        _ => "",
    }
}
