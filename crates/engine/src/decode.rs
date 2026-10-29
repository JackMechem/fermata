//! Reading audio files in any common format and converting them to the project's sample rate.

use std::path::Path;

use anyhow::{anyhow, Context, Result};

/// File extensions the importer accepts.
pub const AUDIO_EXTENSIONS: &[&str] = &["wav", "wave", "aif", "aiff", "aifc", "flac", "mp3", "ogg", "oga", "m4a", "mp4", "aac", "caf"];

pub fn is_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| AUDIO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

pub struct Decoded {
    pub sample_rate: u32,
    /// Up to two channels: stereo is the widest layout tracks play back.
    pub channels: Vec<Vec<f32>>,
    /// True when the file is a WAV that can be referenced as-is.
    pub is_wav: bool,
}

/// Decodes a whole file into memory. WAV goes through hound (fast path); everything else
/// through symphonia.
pub fn decode(path: &Path) -> Result<Decoded> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if ext == "wav" || ext == "wave" {
        if let Ok(d) = decode_wav(path) {
            return Ok(d);
        }
    }
    decode_symphonia(path)
}

fn decode_wav(path: &Path) -> Result<Decoded> {
    let mut reader = hound::WavReader::open(path).with_context(|| format!("opening {}", path.display()))?;
    let spec = reader.spec();
    let ch = spec.channels as usize;
    if ch == 0 {
        return Err(anyhow!("file has no channels"));
    }
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader.samples::<i32>().map(|s| s.map(|v| v as f32 * scale)).collect::<Result<_, _>>()?
        }
    };
    Ok(Decoded { sample_rate: spec.sample_rate, channels: deinterleave(&interleaved, ch), is_wav: true })
}

fn deinterleave(interleaved: &[f32], ch: usize) -> Vec<Vec<f32>> {
    let keep = ch.min(2);
    let frames = interleaved.len() / ch;
    let mut out = vec![Vec::with_capacity(frames); keep];
    for f in interleaved.chunks_exact(ch) {
        for (c, o) in out.iter_mut().enumerate() {
            o.push(f[c]);
        }
    }
    out
}

fn decode_symphonia(path: &Path) -> Result<Decoded> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
    use symphonia::core::errors::Error;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let file = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        // Gapless trims MP3/AAC encoder delay and padding, so imports line up with the beat.
        .format(&hint, mss, &FormatOptions { enable_gapless: true, ..Default::default() }, &MetadataOptions::default())
        .map_err(|e| anyhow!("unsupported or damaged audio file ({e})"))?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| anyhow!("no audio stream in file"))?;
    let track_id = track.id;
    let mut sample_rate = track.codec_params.sample_rate.unwrap_or(0);
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| anyhow!("no decoder for this format ({e})"))?;

    let mut channels: Vec<Vec<f32>> = Vec::new();
    let mut buf: Option<SampleBuffer<f32>> = None;
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(Error::ResetRequired) => break,
            Err(e) => return Err(anyhow!("reading {}: {e}", path.display())),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            // Corrupt frames are skipped, as players do.
            Err(Error::DecodeError(_)) => continue,
            Err(e) => return Err(anyhow!("decoding {}: {e}", path.display())),
        };
        let spec = *decoded.spec();
        sample_rate = spec.rate;
        let n = spec.channels.count().max(1);
        if buf.as_ref().map(|b| b.capacity() < decoded.capacity() * n).unwrap_or(true) {
            buf = Some(SampleBuffer::new(decoded.capacity() as u64, spec));
        }
        let b = buf.as_mut().unwrap();
        b.copy_interleaved_ref(decoded);
        if channels.is_empty() {
            channels = vec![Vec::new(); n.min(2)];
        }
        for f in b.samples().chunks_exact(n) {
            for (c, o) in channels.iter_mut().enumerate() {
                o.push(f[c]);
            }
        }
    }
    if channels.is_empty() || channels[0].is_empty() || sample_rate == 0 {
        return Err(anyhow!("no audio decoded from {}", path.display()));
    }
    Ok(Decoded { sample_rate, channels, is_wav: false })
}

/// High-quality sample-rate conversion (windowed sinc). Returns the input when rates match.
pub fn resample(channels: Vec<Vec<f32>>, from: u32, to: u32) -> Result<Vec<Vec<f32>>> {
    use rubato::{Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction};
    if from == to || channels.is_empty() {
        return Ok(channels);
    }
    let ratio = to as f64 / from as f64;
    let params = SincInterpolationParameters {
        sinc_len: 256,
        f_cutoff: 0.95,
        interpolation: SincInterpolationType::Cubic,
        oversampling_factor: 256,
        window: WindowFunction::BlackmanHarris2,
    };
    let chunk = 4096;
    let nch = channels.len();
    let mut r = SincFixedIn::<f32>::new(ratio, 1.0, params, chunk, nch).map_err(|e| anyhow!("resampler: {e}"))?;
    let frames = channels[0].len();
    let expected = (frames as f64 * ratio).round() as usize;
    let mut out: Vec<Vec<f32>> = vec![Vec::with_capacity(expected + chunk); nch];
    let push = |o: Vec<Vec<f32>>, out: &mut Vec<Vec<f32>>| {
        for (dst, src) in out.iter_mut().zip(o) {
            dst.extend_from_slice(&src);
        }
    };
    let mut pos = 0;
    while pos + chunk <= frames {
        let input: Vec<&[f32]> = channels.iter().map(|c| &c[pos..pos + chunk]).collect();
        push(r.process(&input, None).map_err(|e| anyhow!("resampling: {e}"))?, &mut out);
        pos += chunk;
    }
    if pos < frames {
        let input: Vec<&[f32]> = channels.iter().map(|c| &c[pos..]).collect();
        push(r.process_partial(Some(&input), None).map_err(|e| anyhow!("resampling: {e}"))?, &mut out);
    }
    // Flush the filter's tail, then drop its delay from the front.
    let delay = r.output_delay();
    while out[0].len() < expected + delay {
        push(r.process_partial::<&[f32]>(None, None).map_err(|e| anyhow!("resampling: {e}"))?, &mut out);
    }
    for c in out.iter_mut() {
        c.drain(..delay.min(c.len()));
        c.truncate(expected);
    }
    Ok(out)
}

/// Writes 32-bit float WAV.
pub fn write_wav(path: &Path, sample_rate: u32, channels: &[Vec<f32>]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let spec = hound::WavSpec { channels: channels.len() as u16, sample_rate, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let mut w = hound::WavWriter::create(path, spec)?;
    let frames = channels.first().map(|c| c.len()).unwrap_or(0);
    for i in 0..frames {
        for c in channels {
            w.write_sample(c[i])?;
        }
    }
    w.finalize()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_keeps_length_and_pitch() {
        // One second of 1 kHz at 44.1 kHz -> 48 kHz should give 48000 frames still at 1 kHz.
        let sine: Vec<f32> = (0..44100).map(|i| (std::f32::consts::TAU * 1000.0 * i as f32 / 44100.0).sin()).collect();
        let out = resample(vec![sine], 44100, 48000).unwrap();
        assert_eq!(out[0].len(), 48000);
        // Count rising zero crossings in the middle to estimate the frequency.
        let s = &out[0][4800..43200];
        let crossings = s.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        let hz = crossings as f64 / (s.len() as f64 / 48000.0);
        assert!((hz - 1000.0).abs() < 3.0, "measured {hz} Hz");
        let peak = s.iter().fold(0.0f32, |a, &b| a.max(b.abs()));
        assert!((peak - 1.0).abs() < 0.02, "peak {peak}");
    }

    #[test]
    fn wav_round_trip() {
        // decode() must read back exactly what write_wav() produces.
        let dir = std::env::temp_dir().join(format!("fermata-decode-{}", std::process::id()));
        let p = dir.join("t.wav");
        let ch = vec![(0..1000).map(|i| i as f32 / 1000.0).collect::<Vec<f32>>(); 2];
        write_wav(&p, 48000, &ch).unwrap();
        let d = decode(&p).unwrap();
        assert_eq!(d.sample_rate, 48000);
        assert_eq!(d.channels.len(), 2);
        assert!((d.channels[1][500] - 0.5).abs() < 1e-6);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
