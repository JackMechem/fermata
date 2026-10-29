//! The system mixer's software volume on the recording device. PipeWire (and PulseAudio)
//! keep a per-source volume that applies before any application sees the signal; anything
//! below 100% silently attenuates recordings, which is a common cause of "my interface is
//! clipping but the DAW is quiet". We read and reset it through WirePlumber's `wpctl`.

use std::process::Command;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct InputLevel {
    /// Human-readable device name.
    pub device: String,
    /// The mixer's volume on its own (cubic) scale, 1.0 = 100%.
    pub volume: f32,
    /// The resulting attenuation in dB (0 at 100%).
    pub gain_db: f32,
    pub muted: bool,
    #[serde(skip)]
    target: String,
}

impl InputLevel {
    /// True when the mixer is lowering or muting the signal.
    pub fn attenuating(&self) -> bool {
        self.muted || self.gain_db < -0.5
    }
}

/// Maps a cpal device id to something `wpctl` understands, or None for devices that don't
/// pass through the sound server (direct ALSA hardware).
fn target_for(device: Option<&str>) -> Option<String> {
    match device {
        None | Some("alsa:default") | Some("alsa:pipewire") | Some("pipewire:input_default") => {
            Some("@DEFAULT_AUDIO_SOURCE@".to_string())
        }
        Some(id) => {
            let name = id.strip_prefix("pipewire:").or_else(|| id.strip_prefix("pulseaudio:"))?;
            node_id_by_name(name)
        }
    }
}

/// Finds a node's numeric id from its `node.name` via `pw-dump`.
fn node_id_by_name(name: &str) -> Option<String> {
    let out = Command::new("pw-dump").output().ok()?;
    let objs: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout).ok()?;
    objs.iter()
        .find(|o| o["info"]["props"]["node.name"].as_str() == Some(name))
        .and_then(|o| o["id"].as_u64())
        .map(|id| id.to_string())
}

fn describe(target: &str) -> String {
    let out = Command::new("wpctl").args(["inspect", target]).output().ok();
    let text = out.map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
    text.lines()
        .find_map(|l| l.trim().trim_start_matches('*').trim().strip_prefix("node.description = "))
        .map(|s| s.trim_matches('"').to_string())
        .unwrap_or_else(|| "Input device".into())
}

pub fn input_level(device: Option<&str>) -> Option<InputLevel> {
    let target = target_for(device)?;
    let out = Command::new("wpctl").args(["get-volume", &target]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    // "Volume: 0.22" or "Volume: 0.22 [MUTED]"
    let text = String::from_utf8_lossy(&out.stdout);
    let volume: f32 = text.split_whitespace().nth(1)?.parse().ok()?;
    let muted = text.contains("MUTED");
    let linear = volume.powi(3);
    let gain_db = if linear <= 0.0 { -120.0 } else { 20.0 * linear.log10() };
    Some(InputLevel { device: describe(&target), volume, gain_db, muted, target })
}

/// Sets the recording device's system volume to 100% and unmutes it.
pub fn reset_input_level(device: Option<&str>) -> Result<Option<InputLevel>, String> {
    let Some(level) = input_level(device) else { return Ok(None) };
    for args in [vec!["set-volume", level.target.as_str(), "1.0"], vec!["set-mute", level.target.as_str(), "0"]] {
        let st = Command::new("wpctl").args(&args).status().map_err(|e| format!("wpctl: {e}"))?;
        if !st.success() {
            return Err(format!("wpctl {} failed", args[0]));
        }
    }
    Ok(input_level(device))
}
