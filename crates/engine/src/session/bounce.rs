//! Bounces and exports, as in Logic's File › Bounce and File › Export menus and Track › Bounce
//! in Place. Everything is an offline render of the real graph (`Engine::render`), written to
//! WAV or placed back into the project.
//!
//! The render step only touches the engine and the disk, so it can run on a worker thread.
//! Bounce in Place's second step (`place_bounces`) edits tracks and plug-ins and belongs on the
//! main thread like any other edit.

use super::*;

/// Where Normalize puts the loudest sample, as Logic does.
const NORMALIZE_PEAK_DB: f32 = -0.1;
/// Below about -90 dBFS a render's tail counts as silent and is trimmed.
const SILENCE: f32 = 3.2e-5;
/// The audio tail Bounce in Place keeps at most.
const IN_PLACE_TAIL_SECS: f64 = 2.0;

/// Sample format of a bounced file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BitDepth {
    /// 16-bit integer, with TPDF dither.
    Int16,
    Int24,
    Float32,
}

/// What to render and how to write it (Bounce, Export All Tracks, Export Region).
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct RenderOptions {
    /// Timeline range in project samples, end exclusive. Export Region uses the region instead.
    pub start: i64,
    pub end: i64,
    /// Rendered after `end` so reverbs and releases ring out, in seconds. Bounces and region
    /// exports cut the silent part off; exported tracks keep it, so they all line up.
    pub tail_secs: f64,
    pub bit_depth: BitDepth,
    /// Output sample rate; None keeps the project's.
    pub sample_rate: Option<u32>,
    /// Scale each file so its peak sits at -0.1 dBFS.
    pub normalize: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions { start: 0, end: 0, tail_secs: 2.0, bit_depth: BitDepth::Int24, sample_rate: None, normalize: false }
    }
}

/// What a bounce or export wrote.
#[derive(Debug, Clone, Serialize)]
pub struct BounceResult {
    pub paths: Vec<String>,
    /// The loudest file's peak, in dBFS.
    pub peak_db: f32,
    /// Length of the (longest) file.
    pub seconds: f64,
}

/// What Bounce in Place does with the material it bounced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BipSource {
    Leave,
    #[default]
    Mute,
    Delete,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct BipOptions {
    /// Render the track's Channel EQ and Audio FX plug-ins.
    pub effects: bool,
    /// Render its fader and pan. When off, the new track takes over the source's settings.
    pub fader: bool,
    pub source: BipSource,
    /// Keep up to two seconds of reverb and release after the last region.
    pub tail: bool,
}

impl Default for BipOptions {
    fn default() -> Self {
        BipOptions { effects: true, fader: false, source: BipSource::Mute, tail: true }
    }
}

/// One track's Bounce in Place render, written to disk and waiting to be placed.
pub struct InPlaceRender {
    source: Id,
    /// Set when only these regions were bounced (from the region menu).
    regions: Option<Vec<Id>>,
    start: i64,
    frames: i64,
    path: PathBuf,
    clip: Clip,
}

impl Session {
    /// Where the last region ends (0 for an empty project): the end of "Whole Project".
    pub fn project_end(&self) -> i64 {
        self.project.tracks.iter().flat_map(|t| t.regions.iter()).map(|r| r.end()).max().unwrap_or(0)
    }

    /// Renders `[from, to)` plus `tail` frames through the engine, as stereo at the project
    /// rate. The audio device plays silence meanwhile. `isolate` renders one track instead of
    /// the full mix; `progress` gets 0..1.
    pub fn render(&mut self, from: i64, to: i64, tail: i64, isolate: Option<Isolate>, progress: &mut dyn FnMut(f64)) -> Result<Vec<Vec<f32>>> {
        if self.rec_punch.is_some() {
            return Err(anyhow!("Stop recording before bouncing"));
        }
        if to <= from {
            return Err(anyhow!("Nothing to bounce: the range is empty"));
        }
        let engine = self.engine.clone();
        let audio = engine.lock().render(from, to, tail, isolate, progress);
        Ok(audio)
    }

    /// The isolate settings for rendering `tap` through its channel strip. Its subtracks play
    /// unless they (or a stack between them and `tap`) are muted; `tap` itself always plays,
    /// whatever its mute and the project's solos say.
    pub fn isolate_track(&self, tap: Id, fader: bool, effects: bool) -> Isolate {
        let p = &self.project;
        let mut tracks = vec![(tap, true)];
        for d in p.descendants(tap) {
            let muted = |id: Id| p.track(id).map(|t| t.mute).unwrap_or(false);
            let mut heard = !muted(d);
            for a in p.ancestors(d).into_iter().take_while(|a| *a != tap) {
                heard &= !muted(a);
            }
            tracks.push((d, heard));
        }
        Isolate { tracks, tap, fader, effects }
    }

    /// The span covered by a track's unmuted regions and its subtracks'.
    fn content_span(&self, id: Id) -> Option<(i64, i64)> {
        let mut family = vec![id];
        family.extend(self.project.descendants(id));
        let regions = family.iter().filter_map(|t| self.project.track(*t)).flat_map(|t| t.regions.iter()).filter(|r| !r.muted);
        span(regions)
    }

    fn tail_frames(&self, secs: f64) -> i64 {
        (secs.clamp(0.0, 60.0) * self.sample_rate()).round() as i64
    }

    /// Resamples and normalizes a render as the options ask, returning it with its rate.
    fn finish(&self, audio: Vec<Vec<f32>>, o: &RenderOptions) -> Result<(Vec<Vec<f32>>, u32)> {
        let sr = self.sample_rate() as u32;
        let rate = o.sample_rate.filter(|r| (8000..=384_000).contains(r)).unwrap_or(sr);
        let mut audio = crate::decode::resample(audio, sr, rate)?;
        if o.normalize {
            normalize(&mut audio, NORMALIZE_PEAK_DB);
        }
        Ok((audio, rate))
    }

    /// File › Bounce › Project or Section: the full mix, through Stereo Out, as one file.
    pub fn bounce_project(&mut self, path: &Path, o: &RenderOptions, progress: &mut dyn FnMut(f64)) -> Result<BounceResult> {
        let tail = self.tail_frames(o.tail_secs);
        let mut audio = self.render(o.start, o.end, tail, None, progress)?;
        trim_tail(&mut audio, (o.end - o.start) as usize);
        let (audio, rate) = self.finish(audio, o)?;
        write_wav(path, rate, &audio, o.bit_depth)?;
        Ok(BounceResult { paths: vec![path.to_string_lossy().to_string()], peak_db: to_db(peak(&audio)), seconds: frames(&audio) as f64 / rate as f64 })
    }

    /// The tracks Export All Tracks writes: every audio and instrument track, plus each summing
    /// stack as its mixed output (its subtracks are also written on their own). Folder stacks
    /// have no channel strip, so they get no file. Tracks with nothing to play are left out.
    pub fn export_targets(&self) -> Vec<Id> {
        self.project
            .tracks
            .iter()
            .filter(|t| t.kind != TrackKind::Folder && self.content_span(t.id).is_some())
            .map(|t| t.id)
            .collect()
    }

    /// File › Export › All Tracks as Audio Files: each target track rendered alone through its
    /// channel strip (fader and pan included, Stereo Out not), named "<number> <track>.wav"
    /// after its place in the track list. All files cover the same range, tail included, so
    /// they line up when imported elsewhere.
    pub fn export_tracks(&mut self, dir: &Path, o: &RenderOptions, progress: &mut dyn FnMut(f64)) -> Result<BounceResult> {
        let targets = self.export_targets();
        if targets.is_empty() {
            return Err(anyhow!("No tracks have regions to export"));
        }
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        let tail = self.tail_frames(o.tail_secs);
        let n = targets.len() as f64;
        let mut result = BounceResult { paths: Vec::new(), peak_db: -f32::INFINITY, seconds: 0.0 };
        for (k, id) in targets.into_iter().enumerate() {
            let Some(index) = self.project.tracks.iter().position(|t| t.id == id) else { continue };
            let name = self.project.tracks[index].name.clone();
            let iso = self.isolate_track(id, true, true);
            let audio = self.render(o.start, o.end, tail, Some(iso), &mut |f| progress((k as f64 + f) / n))?;
            let (audio, rate) = self.finish(audio, o)?;
            let path = dir.join(format!("{:02} {}.wav", index + 1, file_safe(&name)));
            write_wav(&path, rate, &audio, o.bit_depth)?;
            result.peak_db = result.peak_db.max(to_db(peak(&audio)));
            result.seconds = result.seconds.max(frames(&audio) as f64 / rate as f64);
            result.paths.push(path.to_string_lossy().to_string());
        }
        Ok(result)
    }

    /// Export Region as Audio File: the region played alone through its track's Channel EQ
    /// and plug-ins (pre-fader, so the track's level and pan don't colour the file). A muted
    /// region is exported too.
    pub fn export_region(&mut self, id: Id, path: &Path, o: &RenderOptions, progress: &mut dyn FnMut(f64)) -> Result<BounceResult> {
        let (track, idx) = self.find_region(id).ok_or_else(|| anyhow!("no such region"))?;
        let mut t = self.project.track(track).unwrap().clone();
        let r = Region { muted: false, ..t.regions[idx].clone() };
        let (from, to) = (r.start, r.end());
        t.regions = vec![r];
        self.send(Cmd::Regions(track, self.regions_dsp(&t)));
        let iso = self.isolate_track(track, false, true);
        let tail = self.tail_frames(o.tail_secs);
        let rendered = self.render(from, to, tail, Some(iso), progress);
        self.push_regions(track);
        let mut audio = rendered?;
        trim_tail(&mut audio, (to - from) as usize);
        let (audio, rate) = self.finish(audio, o)?;
        write_wav(path, rate, &audio, o.bit_depth)?;
        Ok(BounceResult { paths: vec![path.to_string_lossy().to_string()], peak_db: to_db(peak(&audio)), seconds: frames(&audio) as f64 / rate as f64 })
    }

    /// Bounce in Place, step one: renders each track in `ids` (or, when `regions` is given,
    /// just those regions on their tracks) through its channel strip into the project's Audio
    /// Files folder. A track whose stack is also being bounced goes with the stack.
    pub fn render_in_place(&mut self, ids: &[Id], regions: &[Id], o: &BipOptions, progress: &mut dyn FnMut(f64)) -> Result<Vec<InPlaceRender>> {
        let mut jobs: Vec<(Id, Option<Vec<Id>>)> = Vec::new();
        if regions.is_empty() {
            for t in self.project.tracks.iter().filter(|t| ids.contains(&t.id)) {
                if !self.project.ancestors(t.id).iter().any(|a| ids.contains(a)) {
                    jobs.push((t.id, None));
                }
            }
        } else {
            for t in &self.project.tracks {
                let mine: Vec<Id> = t.regions.iter().filter(|r| regions.contains(&r.id)).map(|r| r.id).collect();
                if !mine.is_empty() {
                    jobs.push((t.id, Some(mine)));
                }
            }
        }
        let sr = self.sample_rate();
        let tail = if o.tail { self.tail_frames(IN_PLACE_TAIL_SECS) } else { 0 };
        let n = jobs.len() as f64;
        let mut out = Vec::new();
        for (k, (source, only)) in jobs.into_iter().enumerate() {
            let Some(mut t) = self.project.track(source).cloned() else { continue };
            let range = match &only {
                None => self.content_span(source),
                Some(keep) => span(t.regions.iter().filter(|r| keep.contains(&r.id))),
            };
            let Some((from, to)) = range else { continue };
            if let Some(keep) = &only {
                // Only the chosen regions play for the render, muted or not.
                t.regions.retain(|r| keep.contains(&r.id));
                t.regions.iter_mut().for_each(|r| r.muted = false);
                self.send(Cmd::Regions(source, self.regions_dsp(&t)));
            }
            let iso = self.isolate_track(source, o.fader, o.effects);
            let rendered = self.render(from, to, tail, Some(iso), &mut |f| progress((k as f64 + f) / n));
            if only.is_some() {
                self.push_regions(source);
            }
            let mut audio = rendered?;
            trim_tail(&mut audio, (to - from) as usize);
            let path = self.unique_audio_path(&file_safe(&format!("{} (bounced)", t.name)));
            crate::decode::write_wav(&path, sr as u32, &audio)?;
            let frames = frames(&audio) as i64;
            out.push(InPlaceRender { source, regions: only, start: from, frames, clip: Clip::from_channels(path.clone(), sr as u32, audio), path });
        }
        if out.is_empty() {
            return Err(anyhow!("Nothing to bounce: there are no regions"));
        }
        Ok(out)
    }

    /// Bounce in Place, step two: a new stereo audio track below each source (in the same
    /// stack, same colour) holding the bounce as one region, then the source is muted or
    /// deleted as asked. Returns the new tracks. Record an undo step first, capturing plug-ins
    /// when the source is deleted.
    pub fn place_bounces(&mut self, renders: Vec<InPlaceRender>, o: &BipOptions) -> Result<Vec<Id>> {
        let mut made = Vec::new();
        for b in renders {
            let Some(src) = self.project.track(b.source).cloned() else { continue };
            let clip = self.add_clip(&b.path, b.clip);
            let mut family = vec![src.id];
            family.extend(self.project.descendants(src.id));
            let below = self.project.tracks.iter().rposition(|t| family.contains(&t.id)).map(|i| i + 1);
            let id = self.add_track(TrackKind::Audio, InputSel::Stereo(0), None, below)?;
            let name = format!("{} (bounced)", src.name);
            let region = Region {
                id: self.project.alloc_id(),
                name: name.clone(),
                start: b.start,
                length: b.frames,
                muted: false,
                color: None,
                content: RegionContent::Audio { clip, offset: 0, gain_db: 0.0, fade_in: 0, fade_out: 0 },
                loop_end: None,
                takes: None,
            };
            let t = self.project.track_mut(id).unwrap();
            t.name = name;
            t.color = src.color.clone();
            t.parent = src.parent;
            // The level and pan weren't printed, so the new track keeps applying them. A folder's
            // fader reaches its subtracks' own gains, so it is always printed.
            if !o.fader && src.kind != TrackKind::Folder {
                t.volume_db = src.volume_db;
                t.pan = src.pan;
            }
            t.regions.push(region);
            self.push_regions(id);
            match (o.source, b.regions) {
                (BipSource::Leave, _) => {}
                (BipSource::Mute, None) => self.project.track_mut(src.id).unwrap().mute = true,
                (BipSource::Mute, Some(ids)) => {
                    for r in self.project.track_mut(src.id).unwrap().regions.iter_mut().filter(|r| ids.contains(&r.id)) {
                        r.muted = true;
                    }
                    self.push_regions(src.id);
                }
                (BipSource::Delete, None) => self.delete_tracks(&[src.id]),
                (BipSource::Delete, Some(ids)) => self.delete_regions(&ids),
            }
            made.push(id);
        }
        self.sync_tracks();
        self.dirty = true;
        Ok(made)
    }

    /// Both Bounce in Place steps, for callers already on the main thread.
    pub fn bounce_in_place(&mut self, ids: &[Id], regions: &[Id], o: &BipOptions, progress: &mut dyn FnMut(f64)) -> Result<Vec<Id>> {
        let renders = self.render_in_place(ids, regions, o, progress)?;
        self.place_bounces(renders, o)
    }
}

fn span<'a>(regions: impl Iterator<Item = &'a Region>) -> Option<(i64, i64)> {
    regions.fold(None, |acc, r| {
        let (s, e) = (r.start, r.end());
        Some(acc.map_or((s, e), |(a, b): (i64, i64)| (a.min(s), b.max(e))))
    })
}

fn frames(audio: &[Vec<f32>]) -> usize {
    audio.first().map(|c| c.len()).unwrap_or(0)
}

/// The loudest absolute sample.
pub fn peak(audio: &[Vec<f32>]) -> f32 {
    audio.iter().flatten().fold(0.0f32, |m, s| m.max(s.abs()))
}

fn to_db(g: f32) -> f32 {
    if g > 0.0 {
        20.0 * g.log10()
    } else {
        -f32::INFINITY
    }
}

/// Scales the audio so its peak lands on `db` dBFS (silence is left alone).
pub fn normalize(audio: &mut [Vec<f32>], db: f32) {
    let p = peak(audio);
    if p > 1e-9 {
        let g = 10f32.powf(db / 20.0) / p;
        audio.iter_mut().flatten().for_each(|s| *s *= g);
    }
}

/// Cuts trailing silence, never below `keep` frames (the bounced range itself).
fn trim_tail(audio: &mut [Vec<f32>], keep: usize) {
    let n = frames(audio);
    let last = (keep..n).rev().find(|&i| audio.iter().any(|c| c[i].abs() > SILENCE)).map(|i| i + 1).unwrap_or(keep);
    audio.iter_mut().for_each(|c| c.truncate(last.min(n)));
}

/// A track or region name made safe as a file name on every platform.
fn file_safe(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { '_' } else { c }).collect();
    let s = s.trim().trim_end_matches('.').to_string();
    if s.is_empty() {
        "Untitled".into()
    } else {
        s
    }
}

/// Writes interleaved WAV in the given format. 16-bit gets TPDF dither (±1 LSB triangular
/// noise) so quiet passages fade into noise rather than distortion; the integer formats clip
/// at full scale.
pub fn write_wav(path: &Path, rate: u32, audio: &[Vec<f32>], depth: BitDepth) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let (bits, format) = match depth {
        BitDepth::Int16 => (16, hound::SampleFormat::Int),
        BitDepth::Int24 => (24, hound::SampleFormat::Int),
        BitDepth::Float32 => (32, hound::SampleFormat::Float),
    };
    let spec = hound::WavSpec { channels: audio.len().max(1) as u16, sample_rate: rate, bits_per_sample: bits, sample_format: format };
    let mut w = hound::WavWriter::create(path, spec).with_context(|| format!("creating {}", path.display()))?;
    // xorshift32: plenty for dither noise, and keeps renders repeatable.
    let mut seed: u32 = 0x9e37_79b9;
    let mut uniform = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as f32 / u32::MAX as f32
    };
    for i in 0..frames(audio) {
        for c in audio {
            let s = c[i];
            match depth {
                BitDepth::Int16 => {
                    let v = s * 32767.0 + (uniform() - uniform());
                    w.write_sample(v.round().clamp(-32768.0, 32767.0) as i16)?;
                }
                BitDepth::Int24 => w.write_sample((s as f64 * 8_388_607.0).round().clamp(-8_388_608.0, 8_388_607.0) as i32)?,
                BitDepth::Float32 => w.write_sample(s)?,
            }
        }
    }
    w.finalize()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_keeps_range_and_ringing_tail() {
        let mut a = vec![vec![0.0f32; 100]; 2];
        a[1][60] = 0.01;
        trim_tail(&mut a, 40);
        assert_eq!(a[0].len(), 61);
        let mut b = vec![vec![0.0f32; 100]; 2];
        trim_tail(&mut b, 40);
        assert_eq!(b[0].len(), 40);
    }

    #[test]
    fn normalize_hits_target() {
        let mut a = vec![vec![0.1f32, -0.25, 0.2]];
        normalize(&mut a, -0.1);
        assert!((to_db(peak(&a)) + 0.1).abs() < 1e-4);
    }

    #[test]
    fn sixteen_bit_dither_is_small_and_unbiased() {
        let dir = std::env::temp_dir().join(format!("fermata-bounce-{}", std::process::id()));
        let p = dir.join("d.wav");
        let quiet = vec![vec![0.25f32; 48000]];
        write_wav(&p, 48000, &quiet, BitDepth::Int16).unwrap();
        let mut r = hound::WavReader::open(&p).unwrap();
        assert_eq!(r.spec().bits_per_sample, 16);
        let s: Vec<i16> = r.samples::<i16>().map(|x| x.unwrap()).collect();
        let mean = s.iter().map(|&v| v as f64).sum::<f64>() / s.len() as f64;
        assert!((mean - 0.25 * 32767.0).abs() < 0.1, "mean {mean}");
        assert!(s.iter().all(|&v| (v as f32 - 0.25 * 32767.0).abs() <= 1.5));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn names_are_file_safe() {
        assert_eq!(file_safe("Bass/Synth: \"A\""), "Bass_Synth_ _A_");
        assert_eq!(file_safe("  "), "Untitled");
    }
}
