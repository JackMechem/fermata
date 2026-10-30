//! Encoder delay and padding in MP4 and M4A files. An AAC encoder starts every file with
//! priming samples (2112 at 44.1 kHz from Apple's encoder, about 48 ms) and pads the end to a
//! whole frame. symphonia's MP4 reader decodes them as audio, so imports would start late by
//! that much; this reads how much to drop and the decoder trims it exactly.
//!
//! Two places say it: iTunes' `iTunSMPB` tag (moov/udta/meta/ilst, a `----` item), which
//! iTunes, Apple Music and most encoders write, and the audio track's edit list
//! (moov/trak/edts/elst), whose media time is where playback starts. The tag wins when both
//! are there.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// What to trim from a decoded MP4 audio track, in samples at `timescale`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gapless {
    /// Priming samples at the start.
    pub delay: u64,
    /// The real audio's length after the priming, when the file says.
    pub frames: Option<u64>,
    /// Padding at the end, used when the length isn't known.
    pub padding: u64,
    /// Samples per second the numbers are counted in (the track's media timescale; the
    /// `iTunSMPB` tag counts at the sample rate, given as 0).
    pub timescale: u32,
}

impl Gapless {
    /// Drops the priming and padding from decoded channels at `sample_rate`.
    pub fn trim(&self, channels: &mut [Vec<f32>], sample_rate: u32) {
        let scale = |n: u64| if self.timescale == 0 || self.timescale == sample_rate { n } else { (n as f64 * sample_rate as f64 / self.timescale as f64).round() as u64 };
        let len = channels.first().map(|c| c.len()).unwrap_or(0);
        let delay = (scale(self.delay) as usize).min(len);
        let keep = match self.frames {
            Some(f) => scale(f) as usize,
            None => (len - delay).saturating_sub(scale(self.padding) as usize),
        };
        // A length the file can't hold is wrong; keep everything after the priming.
        let keep = if keep == 0 || keep > len - delay { len - delay } else { keep };
        for c in channels.iter_mut() {
            c.drain(..delay.min(c.len()));
            c.truncate(keep);
        }
    }
}

/// Reads the delay and padding of the MP4 file at `path`, or None when it isn't an MP4 file
/// or says nothing about them.
pub fn read(path: &Path) -> Option<Gapless> {
    let mut f = std::fs::File::open(path).ok()?;
    let end = f.metadata().ok()?.len();
    let mut at = 0u64;
    // Walk the top-level boxes to `moov`, which can sit before or after the audio data.
    while at + 8 <= end {
        f.seek(SeekFrom::Start(at)).ok()?;
        let mut head = [0u8; 16];
        f.read_exact(&mut head[..8]).ok()?;
        let mut size = u32::from_be_bytes(head[..4].try_into().unwrap()) as u64;
        let mut header = 8;
        if size == 1 {
            f.read_exact(&mut head[8..]).ok()?;
            size = u64::from_be_bytes(head[8..].try_into().unwrap());
            header = 16;
        } else if size == 0 {
            size = end - at;
        }
        if size < header || at + size > end {
            return None;
        }
        if at == 0 && &head[4..8] != b"ftyp" {
            return None;
        }
        if &head[4..8] == b"moov" {
            // Movie headers are small; anything huge is not one.
            let len = (size - header).min(64 << 20) as usize;
            let mut body = vec![0u8; len];
            f.read_exact(&mut body).ok()?;
            return parse_moov(&body);
        }
        at += size;
    }
    None
}

/// The boxes in `data`: each one's type and body.
fn boxes(data: &[u8]) -> impl Iterator<Item = (&[u8], &[u8])> {
    let mut at = 0usize;
    std::iter::from_fn(move || {
        if at + 8 > data.len() {
            return None;
        }
        let mut size = u32::from_be_bytes(data[at..at + 4].try_into().unwrap()) as usize;
        let kind = &data[at + 4..at + 8];
        let mut header = 8;
        if size == 1 {
            if at + 16 > data.len() {
                return None;
            }
            size = u64::from_be_bytes(data[at + 8..at + 16].try_into().unwrap()).try_into().ok()?;
            header = 16;
        } else if size == 0 {
            size = data.len() - at;
        }
        if size < header || at + size > data.len() {
            return None;
        }
        let body = &data[at + header..at + size];
        at += size;
        Some((kind, body))
    })
}

fn child<'a>(data: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
    boxes(data).find(|(k, _)| *k == kind).map(|(_, b)| b)
}

fn be32(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(d.get(at..at + 4)?.try_into().ok()?))
}

fn be64(d: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_be_bytes(d.get(at..at + 8)?.try_into().ok()?))
}

/// A movie or media header's timescale (`mvhd` and `mdhd` share the layout up to it).
fn timescale(header: &[u8]) -> Option<u32> {
    match header.first()? {
        0 => be32(header, 12),
        1 => be32(header, 20),
        _ => None,
    }
}

/// The first edit of an edit list that plays media: (segment duration, media time).
fn first_edit(elst: &[u8]) -> Option<(u64, i64)> {
    let version = *elst.first()?;
    let count = be32(elst, 4)? as usize;
    let mut at = 8;
    for _ in 0..count {
        let (dur, time, step) = match version {
            0 => (be32(elst, at)? as u64, be32(elst, at + 4)? as i32 as i64, 12),
            1 => (be64(elst, at)?, be64(elst, at + 8)? as i64, 20),
            _ => return None,
        };
        // An empty edit (media time −1) only delays the start; skip it.
        if time >= 0 {
            return Some((dur, time));
        }
        at += step;
    }
    None
}

/// The `iTunSMPB` tag's value, from `meta`'s body.
fn itunsmpb(meta: &[u8]) -> Option<String> {
    // `meta` is a full box in MP4 files but a plain one in QuickTime files: in those its
    // first child (an `hdlr`) starts right away.
    let body = if meta.get(4..8) == Some(b"hdlr") { meta } else { meta.get(4..)? };
    let ilst = child(body, b"ilst")?;
    for (kind, item) in boxes(ilst) {
        if kind != b"----" {
            continue;
        }
        let name = child(item, b"name").and_then(|n| n.get(4..));
        if name != Some(b"iTunSMPB".as_slice()) {
            continue;
        }
        // `data`: type and locale, then the text.
        let data = child(item, b"data")?.get(8..)?;
        return Some(String::from_utf8_lossy(data).into_owned());
    }
    None
}

/// Parses an `iTunSMPB` value: hex words, the second the priming, the third the padding and
/// the fourth the real length.
fn parse_itunsmpb(text: &str) -> Option<Gapless> {
    let words: Vec<u64> = text.split_whitespace().map(|w| u64::from_str_radix(w, 16)).collect::<Result<_, _>>().ok()?;
    if words.len() < 4 {
        return None;
    }
    Some(Gapless { delay: words[1], padding: words[2], frames: (words[3] > 0).then_some(words[3]), timescale: 0 })
}

/// Reads the delay and padding from a `moov` box's body.
pub fn parse_moov(moov: &[u8]) -> Option<Gapless> {
    let tag = child(moov, b"udta").and_then(|u| child(u, b"meta")).and_then(itunsmpb).and_then(|t| parse_itunsmpb(&t));
    if tag.is_some() {
        return tag;
    }
    let movie_scale = child(moov, b"mvhd").and_then(timescale)?;
    // The first sound track.
    let trak = boxes(moov).filter(|(k, _)| *k == b"trak").map(|(_, b)| b).find(|t| {
        child(t, b"mdia").and_then(|m| child(m, b"hdlr")).and_then(|h| h.get(8..12)) == Some(b"soun".as_slice())
    })?;
    let media_scale = child(trak, b"mdia").and_then(|m| child(m, b"mdhd")).and_then(timescale)?;
    let (dur, time) = child(trak, b"edts").and_then(|e| child(e, b"elst")).and_then(first_edit)?;
    if movie_scale == 0 || media_scale == 0 {
        return None;
    }
    let frames = (dur > 0).then(|| (dur as f64 * media_scale as f64 / movie_scale as f64).round() as u64);
    Some(Gapless { delay: time as u64, frames, padding: 0, timescale: media_scale })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bx(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut v = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        v.extend_from_slice(kind);
        v.extend_from_slice(body);
        v
    }

    fn cat(parts: &[Vec<u8>]) -> Vec<u8> {
        parts.concat()
    }

    /// A version 0 `mvhd`/`mdhd` with `scale`.
    fn header(kind: &[u8; 4], scale: u32) -> Vec<u8> {
        let mut b = vec![0u8; 12];
        b.extend_from_slice(&scale.to_be_bytes());
        b.extend_from_slice(&[0u8; 4]);
        bx(kind, &b)
    }

    fn trak(handler: &[u8; 4], media_scale: u32, edits: &[(u32, i32)]) -> Vec<u8> {
        let mut hdlr = vec![0u8; 8];
        hdlr.extend_from_slice(handler);
        hdlr.extend_from_slice(&[0u8; 12]);
        let mut elst = vec![0u8; 4];
        elst.extend_from_slice(&(edits.len() as u32).to_be_bytes());
        for &(d, t) in edits {
            elst.extend_from_slice(&d.to_be_bytes());
            elst.extend_from_slice(&t.to_be_bytes());
            elst.extend_from_slice(&[0, 1, 0, 0]);
        }
        let mdia = bx(b"mdia", &cat(&[header(b"mdhd", media_scale), bx(b"hdlr", &hdlr)]));
        bx(b"trak", &cat(&[bx(b"tkhd", &[0u8; 84]), bx(b"edts", &bx(b"elst", &elst)), mdia]))
    }

    fn smpb_udta(text: &str, full_meta: bool) -> Vec<u8> {
        let mut name = vec![0u8; 4];
        name.extend_from_slice(b"iTunSMPB");
        let mut mean = vec![0u8; 4];
        mean.extend_from_slice(b"com.apple.iTunes");
        let mut data = vec![0, 0, 0, 1, 0, 0, 0, 0];
        data.extend_from_slice(text.as_bytes());
        let item = bx(b"----", &cat(&[bx(b"mean", &mean), bx(b"name", &name), bx(b"data", &data)]));
        let other = bx(b"\xa9nam", &bx(b"data", &[0, 0, 0, 1, 0, 0, 0, 0, b'x']));
        let ilst = bx(b"ilst", &cat(&[other, item]));
        let hdlr = bx(b"hdlr", &[0u8; 25]);
        let meta_body = if full_meta { cat(&[vec![0u8; 4], hdlr, ilst]) } else { cat(&[hdlr, ilst]) };
        bx(b"udta", &bx(b"meta", &meta_body))
    }

    const SMPB: &str = " 00000000 00000840 000001CA 00000000003F31F6 00000000 00000000 00000000 00000000";

    #[test]
    fn reads_itunsmpb() {
        for full in [true, false] {
            let moov = cat(&[header(b"mvhd", 600), trak(b"soun", 44_100, &[(1_000, 0)]), smpb_udta(SMPB, full)]);
            let g = parse_moov(&moov).unwrap();
            assert_eq!(g, Gapless { delay: 2112, padding: 0x1CA, frames: Some(0x3F31F6), timescale: 0 });
        }
    }

    #[test]
    fn falls_back_to_the_sound_tracks_edit_list() {
        // A video track first, then the sound track: an empty edit, then the media from
        // 2112 for 10 s of the 600-per-second movie timescale.
        let moov = cat(&[header(b"mvhd", 600), trak(b"vide", 600, &[(6_000, 0)]), trak(b"soun", 44_100, &[(300, -1), (6_000, 2112)])]);
        let g = parse_moov(&moov).unwrap();
        assert_eq!(g, Gapless { delay: 2112, frames: Some(441_000), padding: 0, timescale: 44_100 });
        // No edit list, no tag: nothing to say.
        let moov = cat(&[header(b"mvhd", 600), trak(b"soun", 44_100, &[])]);
        assert_eq!(parse_moov(&moov), None);
        // A damaged tag falls through to the edit list.
        let moov = cat(&[header(b"mvhd", 600), trak(b"soun", 48_000, &[(600, 1024)]), smpb_udta("garbage", true)]);
        assert_eq!(parse_moov(&moov).unwrap().delay, 1024);
    }

    #[test]
    fn trims_priming_and_padding() {
        let ramp = |n: usize| vec![(0..n).map(|i| i as f32).collect::<Vec<f32>>(); 2];
        let mut ch = ramp(10_000);
        Gapless { delay: 2112, frames: Some(5_000), padding: 0, timescale: 0 }.trim(&mut ch, 44_100);
        assert_eq!((ch[0].len(), ch[1][0]), (5_000, 2112.0));
        let mut ch = ramp(10_000);
        Gapless { delay: 100, frames: None, padding: 900, timescale: 0 }.trim(&mut ch, 44_100);
        assert_eq!((ch[0].len(), ch[0][0], *ch[0].last().unwrap()), (9_000, 100.0, 9_099.0));
        // In a media timescale other than the decoded rate.
        let mut ch = ramp(10_000);
        Gapless { delay: 1_000, frames: Some(2_000), padding: 0, timescale: 22_050 }.trim(&mut ch, 44_100);
        assert_eq!((ch[0].len(), ch[0][0]), (4_000, 2_000.0));
        // A length past the end keeps what there is.
        let mut ch = ramp(3_000);
        Gapless { delay: 2112, frames: Some(1_000_000), padding: 0, timescale: 0 }.trim(&mut ch, 44_100);
        assert_eq!(ch[0].len(), 888);
    }

    #[test]
    fn reads_a_file_with_moov_after_the_data() {
        let dir = std::env::temp_dir().join(format!("fermata-gapless-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.m4a");
        let moov = bx(b"moov", &cat(&[header(b"mvhd", 600), trak(b"soun", 44_100, &[(600, 2112)])]));
        let file = cat(&[bx(b"ftyp", b"M4A \0\0\0\0M4A mp42isom"), bx(b"mdat", &[7u8; 5000]), moov]);
        std::fs::write(&path, file).unwrap();
        assert_eq!(read(&path).map(|g| (g.delay, g.frames)), Some((2112, Some(44_100))));
        std::fs::write(&path, b"RIFF....WAVEfmt ").unwrap();
        assert_eq!(read(&path), None);
        std::fs::remove_dir_all(&dir).ok();
    }
}
