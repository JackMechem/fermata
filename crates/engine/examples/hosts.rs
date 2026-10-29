//! Lists every available audio host and its devices.
fn main() {
    for id in cpal::available_hosts() {
        let list = daw_engine::audio_io::list_devices(Some(id.name()));
        println!("== {} ({} in / {} out)", id.name(), list.inputs.len(), list.outputs.len());
        for d in &list.inputs {
            println!("   in : {:<45} ch={} rates={:?} default={} id={}", d.name, d.channels, d.sample_rates, d.is_default, d.id);
        }
        for d in &list.outputs {
            println!("   out: {:<45} ch={} default={} id={}", d.name, d.channels, d.is_default, d.id);
        }
    }
}
