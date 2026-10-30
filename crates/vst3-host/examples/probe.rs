//! Scans installed VST3 plugins, instantiates each one and runs a few blocks through it.
use vst3_host::scan::{default_search_paths, find_bundles, scan_bundle};
use vst3_host::{instantiate, MidiEvent, TransportInfo};

fn main() {
    let filter = std::env::args().nth(1);
    // An optional second argument: only the classes whose name has this in it.
    let only = std::env::args().nth(2);
    let bundles = find_bundles(&default_search_paths());
    for b in bundles {
        if let Some(f) = &filter {
            if !b.to_string_lossy().contains(f.as_str()) {
                continue;
            }
        }
        match scan_bundle(&b) {
            Ok(descs) => {
                for d in descs.iter().filter(|d| only.as_ref().is_none_or(|o| d.name.contains(o.as_str()))).take(if only.is_some() { usize::MAX } else { 3 }) {
                    let (mut p, c) = match instantiate(d, 48000.0, 512) {
                        Ok(x) => x,
                        Err(e) => {
                            println!("  FAIL {} : {e}", d.name);
                            continue;
                        }
                    };
                    let mut l = vec![0.0f32; 512];
                    let mut r = vec![0.0f32; 512];
                    let t = TransportInfo { sample_rate: 48000.0, tempo: 120.0, sig_num: 4, sig_den: 4, ..Default::default() };
                    let mut peak = 0.0f32;
                    for i in 0..20 {
                        for (j, s) in l.iter_mut().enumerate() {
                            *s = ((i * 512 + j) as f32 * 0.05).sin() * 0.5;
                        }
                        r.copy_from_slice(&l);
                        let midi = [MidiEvent { offset: 0, data: [0x90, 60, 100] }];
                        p.process(&mut l, &mut r, if i == 0 { &midi } else { &[] }, &t);
                        peak = peak.max(l.iter().fold(0.0f32, |a, &b| a.max(b.abs())));
                    }
                    for bus in c.audio_buses() {
                        println!("         {} bus \"{}\": {} ch{}{}", if bus.input { "in " } else { "out" }, bus.name, bus.channels, if bus.aux { ", aux" } else { ", main" }, if bus.default_active { ", active by default" } else { "" });
                    }
                    let st = c.save_state();
                    println!("  OK   {:<28} [{}] vendor={} inst={} editor={} state={}B peak={peak:.3}", d.name, d.subcategories, d.vendor, d.is_instrument, c.has_editor(), st.component.len());
                }
                if descs.len() > 3 && only.is_none() {
                    println!("  ... {} more classes in {}", descs.len() - 3, b.display());
                }
            }
            Err(e) => println!("SCAN FAIL {}: {e}", b.display()),
        }
    }
}
