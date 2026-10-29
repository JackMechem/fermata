#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod crash;
mod menu;
mod recent;
#[cfg(target_os = "linux")]
mod glib_loop;
mod settings;

use std::sync::Arc;
use std::time::Duration;

use daw_engine::plugin_db;
use daw_engine::Session;
use parking_lot::Mutex;
use tauri::{Emitter, Manager};

pub struct AppState {
    pub session: Arc<Mutex<Session>>,
}

/// Takes a due autosave on the main thread (plug-in states are read there) and writes it on
/// a worker thread, so the disk never holds up the interface.
fn autosave(s: &mut Session) {
    if let Some(job) = s.autosave_job() {
        std::thread::spawn(move || match job.write() {
            Ok(p) => log::info!("autosaved {}", p.display()),
            Err(e) => log::warn!("autosave failed: {e}"),
        });
    }
}

fn main() {
    // Child-process plug-in scanner: `fermata --scan-vst3 <bundle>`.
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == plugin_db::SCAN_ARG) {
        let code = args.get(i + 1).map(|p| plugin_db::scan_child_main(p.as_ref())).unwrap_or(2);
        std::process::exit(code);
    }

    // Plug-in editors are X11 windows, and GLX-based ones fail when GTK runs natively on Wayland
    // in the same process. Run the UI through XWayland unless the user chose a backend.
    #[cfg(target_os = "linux")]
    if std::env::var_os("GDK_BACKEND").is_none() {
        std::env::set_var("GDK_BACKEND", "x11");
    }

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info,jack=off")).init();

    #[cfg(target_os = "linux")]
    vst3_host::editor::set_event_loop(Box::new(glib_loop::GlibLoop));

    let plugins = plugin_db::load_cache().unwrap_or_else(|| {
        log::info!("scanning plug-ins for the first time");
        std::env::current_exe().map(|exe| plugin_db::scan_with(&exe, &settings::extra_plugin_folders())).unwrap_or_default()
    });
    let prefs = settings::load();
    let mut session = Session::new(prefs.audio, plugins);
    session.set_prefs(prefs.prefs);
    // `fermata path/to/Song.fermata` opens a project at launch.
    if let Some(path) = args.iter().skip(1).find(|a| a.trim_end_matches('/').ends_with(".fermata")) {
        match session.open(std::path::Path::new(path)) {
            Ok(()) => recent::add(&session.project_path()),
            Err(e) => log::error!("could not open {path}: {e}"),
        }
    }
    let session = Arc::new(Mutex::new(session));
    crash::install(session.clone());

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState { session: session.clone() })
        .setup(move |app| {
            menu::install(app)?;
            let handle = app.handle().clone();

            // Meters, playhead and recording previews at ~30 Hz.
            let s = session.clone();
            let h = handle.clone();
            std::thread::Builder::new().name("ui-tick".into()).spawn(move || loop {
                std::thread::sleep(Duration::from_millis(33));
                let tick = s.lock().tick();
                let _ = h.emit("tick", tick);
            })?;

            // Plug-in controller idle + freeing replaced DSP objects, on the main thread.
            #[cfg(target_os = "linux")]
            {
                let s = session.clone();
                let h = handle.clone();
                glib::timeout_add_local(Duration::from_millis(30), move || {
                    if let Some(mut g) = s.try_lock() {
                        if g.idle() {
                            let _ = h.emit("project", commands::snapshot(&g));
                        }
                        autosave(&mut g);
                    }
                    glib::ControlFlow::Continue
                });
            }
            #[cfg(not(target_os = "linux"))]
            {
                let s = session.clone();
                let h = handle.clone();
                std::thread::spawn(move || loop {
                    std::thread::sleep(Duration::from_millis(30));
                    let s = s.clone();
                    let h2 = h.clone();
                    let _ = h.run_on_main_thread(move || {
                        if let Some(mut g) = s.try_lock() {
                            if g.idle() {
                                let _ = h2.emit("project", commands::snapshot(&g));
                            }
                            autosave(&mut g);
                        }
                    });
                });
            }

            // Pick up MIDI devices plugged in after launch.
            let s = session.clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_secs(3));
                s.lock().refresh_midi();
            });

            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_title("Fermata");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::list_plugins,
            commands::rescan_plugins,
            commands::audio_devices,
            commands::get_audio_config,
            commands::set_audio_config,
            commands::play,
            commands::stop,
            commands::toggle_play,
            commands::record,
            commands::locate,
            commands::set_cycle,
            commands::set_tempo,
            commands::set_signature,
            commands::set_key,
            commands::set_metronome,
            commands::set_count_in,
            commands::set_master_volume,
            commands::add_tracks,
            commands::delete_track,
            commands::update_track,
            commands::select_track,
            commands::reorder_tracks,
            commands::move_region,
            commands::resize_region,
            commands::delete_regions,
            commands::set_region_muted,
            commands::rename_region,
            commands::split_regions,
            commands::import_audio,
            commands::import_audio_files,
            commands::midi_event,
            commands::undo,
            commands::redo,
            commands::clip_waveform,
            commands::delete_tracks,
            commands::delete_unused_tracks,
            commands::duplicate_tracks,
            commands::create_stack,
            commands::flatten_stack,
            commands::move_tracks,
            commands::create_group,
            commands::update_group,
            commands::delete_group,
            commands::set_track_group,
            commands::paste_regions,
            commands::repeat_regions,
            commands::set_region_gain,
            commands::normalize_regions,
            commands::set_region_fades,
            commands::set_region_color,
            commands::transpose_regions,
            commands::quantize_regions,
            commands::create_midi_region,
            commands::regions_to_new_track,
            commands::reverse_regions,
            commands::join_regions,
            commands::set_plugin,
            commands::remove_plugin,
            commands::set_bypass,
            commands::open_editor,
            commands::new_project,
            commands::save_project,
            commands::open_project,
            commands::list_dir,
            commands::input_level,
            commands::reset_input_level,
            commands::set_eq,
            commands::set_analyzer,
            commands::recent_projects,
            commands::forget_recent,
            commands::path_exists,
            commands::default_project_dir,
            commands::set_region_loop,
            commands::set_regions_looped,
            commands::convert_loops,
            commands::set_tracks_hidden,
            commands::create_aux,
            commands::set_send,
            commands::set_send_level,
            commands::set_output,
            commands::export_targets,
            commands::bounce_project,
            commands::export_tracks,
            commands::export_region,
            commands::bounce_in_place,
            commands::split_regions_at,
            commands::edit_range,
            commands::clip_transients,
            commands::clip_info,
            commands::set_automation_lane,
            commands::set_auto_mode,
            commands::clear_automation,
            commands::automation_touch,
            commands::list_plugin_params,
            commands::get_settings,
            commands::set_settings,
            commands::plugin_manager,
            commands::restart_audio,
            commands::midi_ports,
            commands::midi_monitor,
            commands::set_click,
            commands::set_record_bit_depth,
            commands::set_project_notes,
            commands::nudge_regions,
            commands::recovery_info,
            commands::recoverable_projects,
            commands::open_recovered,
            commands::discard_autosave,
            commands::set_midi_content,
            commands::import_midi,
            commands::export_midi,
            commands::freeze_tracks,
            commands::unfreeze_tracks,
            commands::add_marker,
            commands::delete_markers,
            commands::rename_marker,
            commands::move_marker,
            commands::set_marker_color,
            commands::set_punch,
            commands::set_comp_range,
            commands::select_take,
            commands::rename_take,
            commands::delete_take,
            commands::delete_unused_takes,
            commands::flatten_take_folder,
            commands::merge_take_folder,
            commands::unpack_take_folder,
            commands::pack_take_folder,
            commands::move_regions,
            commands::clear_undo_history,
            commands::set_tempo_map,
            commands::set_signatures,
            commands::set_tempo_at,
            commands::set_signature_at,
            commands::move_midi_with_tempo,
            commands::list_builtin_effects,
            commands::set_builtin_param,
            commands::builtin_params,
            commands::set_sidechain,
            commands::watch_builtin,
            commands::copy_plugin,
            commands::paste_plugin,
            commands::paste_eq,
            commands::list_presets,
            commands::get_preset,
            commands::load_preset,
            commands::save_preset,
            commands::delete_preset,
            commands::rename_preset,
            commands::move_plugin,
            commands::set_region_fade_shape,
            commands::crossfade_regions,
            commands::set_drag_mode,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Fermata");
}
