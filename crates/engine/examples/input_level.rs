//! Prints (or with `--reset`, restores to 100%) the system mixer volume for a recording device:
//! `--example input_level [--reset] [device-id]`.
#[cfg(target_os = "linux")]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let reset = args.iter().any(|a| a == "--reset");
    let id = args.iter().find(|a| !a.starts_with("--")).cloned();
    if reset {
        println!("{:?}", daw_engine::sysvol::reset_input_level(id.as_deref()));
    } else {
        println!("{:?}", daw_engine::sysvol::input_level(id.as_deref()));
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("input_level reads the ALSA mixer, so it only runs on Linux");
}
