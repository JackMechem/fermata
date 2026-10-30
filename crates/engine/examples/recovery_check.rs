//! Crash recovery against a running engine (silent ALSA null output, no input, so takes are
//! silent and only their lengths and placement are checked).
//!
//! A child process records and is killed with SIGKILL mid-take, as close to a real crash as it
//! gets: no stop, no destructors, the WAV headers as the last checkpoint left them. The parent
//! then finds the work to recover, reopens the project and checks the takes are back where they
//! were recorded, for a plain recording (24-bit, a track that only the record-start autosave
//! knows about) and a cycle recording (32-bit float, a take folder). Last, a normal stop must
//! autosave at once and clear the journal.
//!
//! usage: recovery_check <audio file> <scratch dir>
//! (the audio file is unused; it's there so every check takes the same arguments)
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use daw_engine::audio_io::AudioConfig;
use daw_engine::model::{Id, InputSel, Project, RegionContent, TrackKind};
use daw_engine::prefs::BitDepth;
use daw_engine::session::{recording_journals, recovery_info, TrackPatch, AUTOSAVE_FILE};
use daw_engine::Session;

static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check(name: &str, ok: bool) -> bool {
    FAILED.fetch_or(!ok, std::sync::atomic::Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
    ok
}

fn session(dir: &Path) -> Session {
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = dir.to_path_buf();
    s.prefs.recording.pre_roll_secs = 0.0;
    // Only the immediate autosaves should happen during the check.
    s.prefs.general.autosave_minutes = 10;
    s.set_count_in(false);
    s
}

/// Moves the playhead and waits for the engine to take it: the null device runs flat out,
/// and a block already under way would otherwise report the old position to `record()`.
fn locate(s: &mut Session, pos: i64) {
    s.locate(pos);
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
        if s.position() == pos {
            std::thread::sleep(Duration::from_millis(20));
            if s.position() == pos {
                return;
            }
        }
    }
}

fn arm(s: &mut Session, id: Id) {
    s.update_track(id, TrackPatch { armed: Some(true), ..Default::default() });
}

fn track_named(s: &Session, name: &str) -> Option<Id> {
    s.project.tracks.iter().find(|t| t.name == name).map(|t| t.id)
}

/// The child: sets up a project, records, reports how far the recorder got, then waits to be
/// killed. `mode` is "plain" or "cycle".
fn child(mode: &str, dir: &Path) {
    let mut s = session(dir);
    let sr = s.sample_rate() as i64;
    let vocal = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    s.update_track(vocal, TrackPatch { name: Some("Vocal".into()), ..Default::default() });
    arm(&mut s, vocal);
    let plain = mode == "plain";
    s.set_record_bit_depth(Some(if plain { BitDepth::Int24 } else { BitDepth::Float32 }));
    if !plain {
        let bar = sr * 2; // 4/4 at 120 bpm
        s.set_cycle(true, 0, 2 * bar);
    }
    // Saved, so nothing is unsaved in cycle mode: only the journal tells of the recording.
    s.save(None).unwrap();
    let mut out = std::io::stdout();
    if plain {
        // Only in the autosave written when recording starts.
        let guitar = s.add_track(TrackKind::Audio, InputSel::Stereo(0), None, None).unwrap();
        s.update_track(guitar, TrackPatch { name: Some("Guitar".into()), ..Default::default() });
        arm(&mut s, guitar);
        locate(&mut s, 2 * sr);
    }
    s.record().unwrap();
    writeln!(out, "PUNCH {}", if plain { 2 * sr } else { 0 }).unwrap();
    let want = if plain { 3 * sr } else { 4 * sr * 5 / 2 };
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        std::thread::sleep(Duration::from_millis(2));
        let previews = s.tick().previews;
        let frames = previews.iter().map(|p| p.frames).min().unwrap_or(0) as i64;
        if (frames >= want && (!plain || dir.join(AUTOSAVE_FILE).exists())) || Instant::now() > deadline {
            for p in &previews {
                writeln!(out, "FRAMES {} {}", p.track, p.frames).unwrap();
            }
            break;
        }
    }
    writeln!(out, "READY").unwrap();
    out.flush().unwrap();
    // Keep recording until killed.
    loop {
        std::thread::sleep(Duration::from_millis(5));
        let _ = s.tick();
    }
}

struct Crash {
    punch: i64,
    /// Frames each track's recorder had taken in when the child said READY.
    frames: Vec<(Id, usize)>,
}

/// Runs a child in `mode` and kills it mid-recording.
fn crash(mode: &str, dir: &Path) -> Crash {
    let mut c = Command::new(std::env::current_exe().unwrap())
        .args(["--child", mode, &dir.to_string_lossy()])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn the recording child");
    let stdout = c.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let mut crash = Crash { punch: 0, frames: Vec::new() };
    loop {
        let line = rx.recv_timeout(Duration::from_secs(90)).expect("the recording child died or hung (its error is above)");
        let w: Vec<&str> = line.split_whitespace().collect();
        match w.as_slice() {
            ["PUNCH", p] => crash.punch = p.parse().unwrap(),
            ["FRAMES", t, f] => crash.frames.push((t.parse().unwrap(), f.parse().unwrap())),
            ["READY"] => break,
            _ => {}
        }
    }
    // Let a little more reach the files, then pull the plug.
    std::thread::sleep(Duration::from_millis(50));
    c.kill().unwrap(); // SIGKILL on Unix
    c.wait().unwrap();
    crash
}

fn record_and_stop(s: &mut Session, seconds: i64) {
    let sr = s.sample_rate() as i64;
    s.record().unwrap();
    let start = s.position();
    let deadline = Instant::now() + Duration::from_secs(60);
    while s.position() < start + seconds * sr && Instant::now() < deadline {
        std::thread::sleep(Duration::from_micros(300));
    }
    s.stop();
    std::thread::sleep(Duration::from_millis(60));
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(|a| a == "--child").unwrap_or(false) {
        child(&args[1], Path::new(&args[2]));
        return;
    }
    let scratch = PathBuf::from(args.get(1).expect("usage: recovery_check <audio file> <scratch dir>"));

    // ---- a plain recording, killed mid-take ---------------------------------------------
    let dir = scratch.join("RecoveryCheck.fermata");
    let _ = std::fs::remove_dir_all(&dir);
    let c = crash("plain", &dir);
    check(&format!("the child recorded two takes before the crash ({:?})", c.frames), c.frames.len() == 2 && c.frames.iter().all(|f| f.1 > 0));
    let journals = recording_journals(&dir);
    check(&format!("the recording journal is left behind ({})", journals.len()), journals.len() == 1);
    let j = daw_engine::session::read_journal(&journals[0]).unwrap();
    let mut damaged = 0;
    for t in &j.takes {
        if let Ok(w) = daw_engine::wav_repair::inspect(Path::new(&t.path)) {
            damaged += w.damaged as usize;
        }
    }
    println!("     {damaged} of {} files had stale headers", j.takes.len());

    let info = recovery_info(&dir);
    let ok = info.as_ref().map(|r| r.has_autosave && r.recording.as_ref().map(|x| x.takes == 2).unwrap_or(false)).unwrap_or(false);
    check(&format!("recovery is offered: the record-start autosave and two takes ({:?})", info.as_ref().map(|r| (r.has_autosave, r.recording.clone()))), ok);

    let mut s = session(&scratch.join("Elsewhere.fermata"));
    s.open_recovered(&dir).unwrap();
    let sr = s.sample_rate() as i64;
    check(&format!("the project opens at {}", s.dir.display()), s.dir == dir && s.dirty);
    let mut starts = Vec::new();
    for (name, channels) in [("Vocal", 1), ("Guitar", 2)] {
        let Some(id) = track_named(&s, name) else {
            check(&format!("{name} is in the recovered project"), false);
            continue;
        };
        let t = s.project.track(id).unwrap();
        let reported = c.frames.iter().find(|f| f.0 == id).map(|f| f.1 as i64).unwrap_or(0);
        let Some(r) = t.regions.first() else {
            check(&format!("{name}'s take is recovered"), false);
            continue;
        };
        let RegionContent::Audio { clip, offset, .. } = r.content else { unreachable!() };
        let path = s.project.clips.iter().find(|c| c.id == clip).map(|c| dir.join(&c.path)).unwrap();
        let file = daw_engine::decode::decode(&path).unwrap();
        let frames = file.channels[0].len() as i64;
        starts.push(r.start);
        check(
            &format!("{name}: one take from {} for {} frames, {} channels (recorder had {reported}, file has {frames})", r.start, r.length, file.channels.len()),
            t.regions.len() == 1 && r.length == frames - offset && frames + 4096 >= reported && file.channels.len() == channels && (r.start - 2 * sr).abs() < sr / 10,
        );
        check(&format!("{name}'s file is a valid WAV again"), !daw_engine::wav_repair::inspect(&path).unwrap().damaged);
    }
    check("an autosave is due right after recovery", s.autosave_job().map(|j| j.write().is_ok()).unwrap_or(false));
    check("the journal is gone once the recovered takes are autosaved", recording_journals(&dir).is_empty());

    // ---- a clean stop: autosaved at once, journal removed ---------------------------------
    let bass = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    s.update_track(bass, TrackPatch { name: Some("Bass".into()), ..Default::default() });
    for t in s.project.tracks.iter().map(|t| t.id).collect::<Vec<_>>() {
        s.update_track(t, TrackPatch { armed: Some(t == bass), ..Default::default() });
    }
    // Saved, so the autosave below is the stop's own.
    s.save(None).unwrap();
    locate(&mut s, 2 * sr);
    s.record().unwrap();
    check("a journal is written when recording starts", recording_journals(&dir).len() == 1);
    let start = s.position();
    let deadline = Instant::now() + Duration::from_secs(60);
    while s.position() < start + sr && Instant::now() < deadline {
        std::thread::sleep(Duration::from_micros(300));
    }
    s.stop();
    std::thread::sleep(Duration::from_millis(60));
    let normal = s.project.track(bass).unwrap().regions.first().map(|r| r.start);
    check(&format!("a normal take lands where the recovered ones did ({normal:?} vs {starts:?})"), starts.iter().all(|&x| Some(x) == normal));
    // What the app's main loop does every 30 ms.
    let job = s.autosave_job();
    check("the stop asks for an autosave at once, not after the interval", job.is_some());
    if let Some(job) = job {
        job.write().unwrap();
    }
    let doc: Option<Project> = std::fs::read(dir.join(AUTOSAVE_FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok());
    let has_take = doc.and_then(|d| d.track(bass).map(|t| t.regions.len() == 1)).unwrap_or(false);
    check("the autosave holds the new take", has_take);
    check("a clean stop leaves no journal", recording_journals(&dir).is_empty());
    check("the saved project.json is untouched by the autosave", {
        let saved: Project = serde_json::from_slice(&std::fs::read(dir.join("project.json")).unwrap()).unwrap();
        saved.track(bass).map(|t| t.regions.is_empty()).unwrap_or(false)
    });
    // With the setting off, the stop waits for the interval.
    s.prefs.general.autosave_after_recording = false;
    record_and_stop(&mut s, 1);
    check("with After Recording off, no immediate autosave", s.autosave_job().is_none());
    drop(s);

    // ---- a cycle recording, killed mid-pass ----------------------------------------------
    let dir = scratch.join("RecoveryCycle.fermata");
    let _ = std::fs::remove_dir_all(&dir);
    let c = crash("cycle", &dir);
    let mut s = session(&scratch.join("Elsewhere2.fermata"));
    let info = recovery_info(&dir);
    check(
        &format!("a cycle recording is offered without an autosave ({:?})", info.as_ref().map(|r| (r.has_autosave, r.recording.clone()))),
        info.map(|r| !r.has_autosave && r.recording.is_some()).unwrap_or(false),
    );
    s.open_recovered(&dir).unwrap();
    let sr = s.sample_rate() as i64;
    let cycle = 4 * sr;
    let vocal = track_named(&s, "Vocal").unwrap();
    let regions = &s.project.track(vocal).unwrap().regions;
    let folder = regions.first().and_then(|r| r.takes.as_ref().map(|f| (r.start, r.length, f.takes.len())));
    let passes = c.frames.first().map(|f| f.1 as i64 / cycle).unwrap_or(0);
    check(
        &format!("the cycle passes come back as a take folder over the cycle ({folder:?}, recorder had {passes}+ passes)"),
        regions.len() == 1 && matches!(folder, Some((0, l, n)) if l == cycle && n as i64 >= passes.max(2)),
    );
    let _ = std::fs::remove_dir_all(scratch.join("Elsewhere.fermata"));
    let _ = std::fs::remove_dir_all(scratch.join("Elsewhere2.fermata"));
    if FAILED.load(std::sync::atomic::Ordering::Relaxed) {
        std::process::exit(1);
    }
}
