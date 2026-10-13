pub mod availability_pto;
pub mod availability_rules;
pub mod backup;
pub mod backup_cli;
mod backup_sqlite;
mod commands;
pub mod coverage;
pub mod database;
mod diagnostics;
mod editor_analysis;
mod editor_history;
pub mod employee_restrictions;
pub mod employees;
pub mod entry_intervals;
pub mod error;
pub mod export_cli;
mod export_pdf;
mod export_snapshot;
mod export_xlsx;
mod exports;
pub mod generation;
pub mod generation_diagnostic;
mod generation_inputs;
pub mod generation_snapshot;
pub mod holidays;
pub mod local_values;
pub mod metadata;
pub mod position_restrictions;
pub mod positions;
mod restore;
pub mod rule_facts;
pub mod safe_destination;
pub mod schedule_assignments;
pub mod schedule_editor;
pub mod schedule_write;
pub mod schedules;
mod scheduling_rules;
pub mod settings;
pub mod shift_templates;
pub mod shift_time;
mod startup;
pub mod template_restrictions;
pub mod text;
pub mod validation;
mod validation_snapshot;

use tauri::Manager;

pub fn run() {
    let context = tauri::generate_context!();
    diagnostics::use_fallback_directory(&context.config().identifier);
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        diagnostics::panic(info);
        default_hook(info);
    }));
    // The first plugin exits a second process before setup or any database work.
    let builder =
        tauri::Builder::default().plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let focused = window
                    .unminimize()
                    .and_then(|()| window.show())
                    .and_then(|()| window.set_focus());
                if focused.is_err() {
                    diagnostics::event("window_focus_failed");
                }
            }
        }));
    let builder = commands::register(builder)
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // Existing explicit local CLI operations also pass the first-party
            // instance gate; a second launch cannot bypass it with arguments.
            if commands::run_local_cli() {
                app.handle().cleanup_before_exit();
                std::process::exit(0);
            }
            app.manage(backup::BackupRuntime::default());
            let resolved = database::resolve_database_paths(
                || {
                    app.path().app_local_data_dir().map_err(|_| {
                        error::AppError::new(
                            error::ErrorCode::DatabaseUnavailable,
                            "The application data directory is unavailable.",
                        )
                    })
                },
                cfg!(debug_assertions),
            );
            let context = match resolved {
                Ok(paths) => {
                    diagnostics::use_database_directory(&paths.directory);
                    let outcome = tauri::async_runtime::block_on(startup::start_database(
                        paths,
                        std::sync::Arc::new(metadata::SystemMetadataClock),
                    ));
                    if let Some(state) = outcome.state {
                        app.manage(state);
                    }
                    outcome.context
                }
                Err(error) => startup::StartupContext::unresolved(error),
            };
            diagnostics::startup(&context.report);
            app.manage(context);
            diagnostics::mark_started();
            // A blocked database still has a main shell and preservation IPC.
            Ok(())
        });
    // Tauri's event loop exits the process itself. Returning here means Tauri
    // could not build, or its event loop panicked: window creation (WebView2)
    // runs inside the loop and panics on failure.
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| builder.run(context)));
    let started = diagnostics::started();
    let logged = match &failure {
        Ok(Ok(())) => return,
        Ok(Err(error)) => diagnostics::runtime_failure(started, Some(&format!("{error:?}"))),
        Err(_) => diagnostics::runtime_failure(started, None),
    };
    show_failure(started, logged);
    std::process::exit(1);
}

/// Release builds have no console, so without this the app would vanish silently.
fn show_failure(started: bool, logged: bool) {
    let mut text = String::from(if started {
        "Shift Happens stopped unexpectedly. Open it again to continue."
    } else {
        "Shift Happens couldn't start. Try opening it again. If it still doesn't start, restart Windows and try once more."
    });
    if logged && let Some(file) = diagnostics::log_file() {
        text.push_str("\n\nDetails were saved to the log:\n");
        text.push_str(&safe_destination::display_path(&file));
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            MB_ICONERROR, MB_OK, MB_SETFOREGROUND, MessageBoxW,
        };
        let wide = |value: &str| -> Vec<u16> { value.encode_utf16().chain(Some(0)).collect() };
        let (text, caption) = (wide(&text), wide("Shift Happens"));
        // A new thread owns no windows, so the box's modal loop cannot dispatch
        // messages into the stopped event loop's windows on this thread.
        let shown = std::thread::spawn(move || {
            // SAFETY: both strings are live, null-terminated UTF-16; no owner window.
            unsafe {
                MessageBoxW(
                    std::ptr::null_mut(),
                    text.as_ptr(),
                    caption.as_ptr(),
                    MB_OK | MB_ICONERROR | MB_SETFOREGROUND,
                );
            }
        });
        let _ = shown.join();
    }
    #[cfg(not(windows))]
    eprintln!("{text}");
}
