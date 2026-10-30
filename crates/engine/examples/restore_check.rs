//! Undo, redo, save and reopen with every feature in one project (silent ALSA null output).
//! Each step of `common/all_features.rs` is made, rendered, undone and redone: the undone
//! project must render bit-identically to before the step and the redone one to after it,
//! and the document must match too. Then undoing every step returns the first render, redoing
//! them all the last; the project saves and reopens identically (document and render); and the
//! same project with every field added since 0.1.0-alpha.1 stripped (an older project) still
//! opens, with those fields at their defaults.
//! usage: restore_check <scratch dir>
use std::path::PathBuf;

use daw_engine::audio_io::AudioConfig;
use daw_engine::Session;

#[path = "common/all_features.rs"]
mod all_features;
use all_features::*;

/// Set by any failed check: the example then exits with status 1, so scripts notice.
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, std::sync::atomic::Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

fn render(s: &mut Session) -> Vec<Vec<f32>> {
    s.render(0, LEN, 0, None, &mut |_| {}).unwrap()
}

/// Bit-identical, or where they first differ and by how much at most.
fn diff(a: &[Vec<f32>], b: &[Vec<f32>]) -> Option<(usize, f32)> {
    if a.len() != b.len() || a.iter().zip(b).any(|(x, y)| x.len() != y.len()) {
        return Some((0, f32::INFINITY));
    }
    let mut first = None;
    let mut worst = 0.0f32;
    for (x, y) in a.iter().zip(b) {
        for (i, (p, q)) in x.iter().zip(y).enumerate() {
            if p.to_bits() != q.to_bits() {
                first = Some(first.map_or(i, |f: usize| f.min(i)));
                worst = worst.max((p - q).abs());
            }
        }
    }
    first.map(|f| (f, worst))
}

fn describe(d: Option<(usize, f32)>) -> String {
    match d {
        None => "bit-identical".into(),
        Some((at, worst)) => format!("differs from frame {at} ({:.2} bars), by up to {worst:.2e}", at as f64 / BAR as f64),
    }
}

/// The saved document, without what isn't part of the edit history (see `Session::restore`).
fn doc(s: &Session) -> serde_json::Value {
    let mut v = serde_json::to_value(&s.project).unwrap();
    let o = v.as_object_mut().unwrap();
    o.remove("drag_mode");
    o.remove("alternative");
    v
}

/// The first place two documents differ, as a JSON path.
fn doc_diff(a: &serde_json::Value, b: &serde_json::Value, path: &str) -> Option<String> {
    use serde_json::Value::*;
    match (a, b) {
        (Object(x), Object(y)) => {
            for k in x.keys().chain(y.keys()) {
                let (p, q) = (x.get(k).unwrap_or(&Null), y.get(k).unwrap_or(&Null));
                if let Some(d) = doc_diff(p, q, &format!("{path}.{k}")) {
                    return Some(d);
                }
            }
            None
        }
        (Array(x), Array(y)) if x.len() == y.len() => x.iter().zip(y).enumerate().find_map(|(i, (p, q))| doc_diff(p, q, &format!("{path}[{i}]"))),
        _ if a == b => None,
        _ => Some(format!("{path}: {a} vs {b}")),
    }
}

/// Removes the fields added this release from a saved document, as an older version wrote it.
fn strip_new_fields(v: &mut serde_json::Value) {
    let o = v.as_object_mut().unwrap();
    for k in ["vcas", "alternative", "arrangement", "snap_zero_crossings"] {
        o.remove(k);
    }
    let m = o.get_mut("master").unwrap().as_object_mut().unwrap();
    for k in ["automation", "auto_mode", "auto_lanes"] {
        m.remove(k);
    }
    for t in o.get_mut("tracks").unwrap().as_array_mut().unwrap() {
        let t = t.as_object_mut().unwrap();
        for k in ["vca", "alternatives", "alternative", "auto_lanes"] {
            t.remove(k);
        }
        for r in t.get_mut("regions").unwrap().as_array_mut().unwrap() {
            let r = r.as_object_mut().unwrap();
            r.remove("params");
            r.remove("pattern");
            if let Some(f) = r.get_mut("content").and_then(|c| c.get_mut("Audio")).and_then(|a| a.get_mut("fade_shape")).and_then(|f| f.as_object_mut()) {
                f.remove("loop_xfade");
            }
        }
        for s in t.get_mut("inserts").unwrap().as_array_mut().unwrap() {
            if let Some(s) = s.as_object_mut() {
                s.remove("sidechain_pre");
            }
        }
    }
}

fn main() {
    let scratch = PathBuf::from(std::env::args().nth(1).expect("scratch dir"));
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg.clone(), Vec::new());
    s.dir = scratch.join("Restore.fermata");
    let mut w = base(&mut s, &scratch);

    let first = render(&mut s);
    let first_doc = doc(&s);
    check(&format!("two renders of the same project are the same: {}", describe(diff(&render(&mut s), &first))), diff(&render(&mut s), &first).is_none());
    check("the project sounds", first[0].iter().any(|x| x.abs() > 0.01));

    // Each step: made, undone, redone.
    let steps = steps();
    for (name, step) in &steps {
        let (before, before_doc) = (render(&mut s), doc(&s));
        s.checkpoint(name, None, true);
        step(&mut s, &mut w);
        s.idle();
        let (after, after_doc) = (render(&mut s), doc(&s));
        s.undo();
        let (undone, undone_doc) = (render(&mut s), doc(&s));
        let d = doc_diff(&undone_doc, &before_doc, "");
        check(&format!("{name}: undo restores the document{}", d.as_deref().map(|d| format!(" ({d})")).unwrap_or_default()), d.is_none());
        check(&format!("{name}: undo renders as before ({})", describe(diff(&undone, &before))), diff(&undone, &before).is_none());
        s.redo();
        let redone = render(&mut s);
        let d = doc_diff(&doc(&s), &after_doc, "");
        check(&format!("{name}: redo restores the document{}", d.as_deref().map(|d| format!(" ({d})")).unwrap_or_default()), d.is_none());
        check(&format!("{name}: redo renders as after ({})", describe(diff(&redone, &after))), diff(&redone, &after).is_none());
        if diff(&after, &before).is_none() && after_doc == before_doc {
            println!("  ({name} changed nothing)");
        }
    }

    // Everything undone, then redone.
    let last = render(&mut s);
    let last_doc = doc(&s);
    for _ in &steps {
        s.undo();
    }
    let d = doc_diff(&doc(&s), &first_doc, "");
    check(&format!("undoing every step restores the first document{}", d.as_deref().map(|d| format!(" ({d})")).unwrap_or_default()), d.is_none());
    check(&format!("and the first render ({})", describe(diff(&render(&mut s), &first))), diff(&render(&mut s), &first).is_none());
    for _ in &steps {
        s.redo();
    }
    let d = doc_diff(&doc(&s), &last_doc, "");
    check(&format!("redoing every step restores the last document{}", d.as_deref().map(|d| format!(" ({d})")).unwrap_or_default()), d.is_none());
    check(&format!("and the last render ({})", describe(diff(&render(&mut s), &last))), diff(&render(&mut s), &last).is_none());

    // Save and reopen, in this session and in a new one.
    let saved = s.save(Some(scratch.join("All.fermata"))).unwrap();
    s.open(&saved).unwrap();
    // Save As names the project after its new folder.
    let named = |mut v: serde_json::Value| {
        v.as_object_mut().unwrap().remove("name");
        v
    };
    let d = doc_diff(&named(doc(&s)), &named(last_doc.clone()), "");
    check(&format!("the project reopens with the same document{}", d.as_deref().map(|d| format!(" ({d})")).unwrap_or_default()), d.is_none());
    check(&format!("and the same render ({})", describe(diff(&render(&mut s), &last))), diff(&render(&mut s), &last).is_none());
    drop(s);
    let mut t = Session::new(cfg.clone(), Vec::new());
    t.open(&saved).unwrap();
    check(&format!("a new session renders it the same ({})", describe(diff(&render(&mut t), &last))), diff(&render(&mut t), &last).is_none());

    // An older project: the first document without this release's fields.
    let old_dir = scratch.join("Old.fermata");
    std::fs::create_dir_all(&old_dir).unwrap();
    let mut v = first_doc.clone();
    strip_new_fields(&mut v);
    // Its audio lives with the base project.
    for c in v["clips"].as_array_mut().unwrap() {
        let p = c["path"].as_str().unwrap().to_string();
        if !std::path::Path::new(&p).is_absolute() {
            c["path"] = serde_json::json!(scratch.join("Base.fermata").join(p).to_string_lossy());
        }
    }
    let text = serde_json::to_string_pretty(&v).unwrap();
    check("the older document has none of the new fields", !["\"vcas\"", "\"arrangement\"", "\"auto_lanes\"", "\"snap_zero_crossings\""].iter().any(|k| text.contains(k)));
    std::fs::write(old_dir.join("project.json"), text).unwrap();
    let opened = t.open(&old_dir);
    check(&format!("an older project opens ({:?})", opened.as_ref().err()), opened.is_ok());
    let mut back = doc(&t);
    let mut want = first_doc.clone();
    // Paths and the project's name come from where it was opened.
    for x in [&mut back, &mut want] {
        x.as_object_mut().unwrap().remove("clips");
        x.as_object_mut().unwrap().remove("name");
    }
    let d = doc_diff(&back, &want, "");
    check(&format!("with every new field at its default{}", d.as_deref().map(|d| format!(" ({d})")).unwrap_or_default()), d.is_none());
    check(&format!("and it renders as before ({})", describe(diff(&render(&mut t), &first))), diff(&render(&mut t), &first).is_none());

    if FAILED.load(std::sync::atomic::Ordering::Relaxed) {
        std::process::exit(1);
    }
}
