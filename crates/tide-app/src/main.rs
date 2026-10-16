// Hide the console window on Windows release builds. Dev builds keep it so
// tracing output stays visible.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;

use tauri::Manager;

/// Frontend liveness beacon.
///
/// WebView2 renders in a separate process, so `PrintWindow` captures the host
/// window as black and screenshots cannot confirm the page actually rendered.
/// This gives us a headless, unambiguous signal that the document parsed, the
/// Svelte app mounted, and the IPC bridge round-tripped.
#[tauri::command]
fn ui_ready(detail: String) {
    tracing::info!(target: "tide::ui", "UI_READY {detail}");
}

/// Debug builds load the Vite dev server rather than bundled assets, so opening
/// this binary without one running renders WebView2's "can't reach this page"
/// error -- which looks like a broken app rather than a missing dev server.
/// Fail loudly and say what to do instead.
#[cfg(debug_assertions)]
fn require_dev_server() {
    use std::net::{TcpStream, ToSocketAddrs};
    use std::time::Duration;

    // Vite binds to `localhost`, which on this machine resolves to ::1 *only*.
    // Probing 127.0.0.1 would report "no dev server" while one is running, so
    // resolve the name and try every address it yields.
    const DEV_HOST: &str = "localhost:5174";

    if let Ok(addrs) = DEV_HOST.to_socket_addrs() {
        for addr in addrs {
            if TcpStream::connect_timeout(&addr, Duration::from_millis(750)).is_ok() {
                return;
            }
        }
    }

    let message = concat!(
        "This is a DEBUG build, which loads its UI from the Vite dev server ",
        "at http://localhost:5174 -- and nothing is listening there.\n\n",
        "Run the app with:    ./scripts/dev.sh\n",
        "Or build a standalone version:    ./scripts/release.sh\n\n",
        "The release build embeds the UI and runs on its own.",
    );

    tracing::error!("{message}");
    message_box("Tide - dev server not running", message);
    std::process::exit(1);
}

#[cfg(debug_assertions)]
fn message_box(caption: &str, text: &str) {
    use std::ffi::c_void;
    use std::iter::once;
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "user32")]
    extern "system" {
        fn MessageBoxW(hwnd: *mut c_void, text: *const u16, caption: *const u16, ty: u32) -> i32;
    }

    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(once(0))
            .collect()
    }

    const MB_ICONERROR: u32 = 0x10;

    // SAFETY: both strings are NUL-terminated and outlive the call.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            wide(text).as_ptr(),
            wide(caption).as_ptr(),
            MB_ICONERROR,
        );
    }
}

/// The log, `%APPDATA%\tide\tide.log`, opened to be added to.
///
/// A release build has no console, so without this whatever went wrong went
/// unrecorded. Kept small rather than rotated on a schedule: past a megabyte
/// at launch, it becomes `tide.log.old` and a new one is begun.
fn log_file() -> Option<std::fs::File> {
    const KEPT: u64 = 1024 * 1024;

    let path = tide_core::data_dir().ok()?.join("tide.log");
    if std::fs::metadata(&path).is_ok_and(|log| log.len() > KEPT) {
        let _ = std::fs::rename(&path, path.with_extension("log.old"));
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .ok()
}

fn start_logging() {
    use tracing_subscriber::prelude::*;

    let filter =
        tracing_subscriber::EnvFilter::try_from_env("TIDE_LOG").unwrap_or_else(|_| "info".into());
    let to_file = log_file().map(|file| {
        tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_writer(std::sync::Mutex::new(file))
    });
    // The console too, where there is one: see the top of this file.
    let to_console = cfg!(debug_assertions).then(tracing_subscriber::fmt::layer);
    tracing_subscriber::registry()
        .with(filter)
        .with(to_file)
        .with(to_console)
        .init();

    // A release build aborts on a panic, and this is all that will say why.
    let report = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        tracing::error!("PANIC {panic}");
        report(panic);
    }));
}

/// Say so in the log when the window stops answering, and when the page
/// leaves a terminal's output undrawn.
///
/// Windows closes a window that does not answer it, and leaves behind only
/// the fact that it did. This leaves which of the two had stopped, and for a
/// terminal, how far behind the screen was.
fn watch(app: tauri::AppHandle) {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    /// Longer than anything the window does on purpose.
    const STALLED: Duration = Duration::from_secs(4);

    std::thread::spawn(move || {
        let start = Instant::now();
        let now = move || start.elapsed().as_millis() as u64;
        // When the window's thread last ran something it was asked to, and
        // whether it is being asked now: one question at a time, so a window
        // that is stuck is not left a queue of them to answer.
        let answered = Arc::new(AtomicU64::new(0));
        let asking = Arc::new(AtomicBool::new(false));
        let (mut window_stalled, mut page_stalled) = (false, false);

        loop {
            std::thread::sleep(Duration::from_secs(1));

            if !asking.swap(true, Ordering::SeqCst) {
                let (answered, asking) = (Arc::clone(&answered), Arc::clone(&asking));
                let asked = app.run_on_main_thread(move || {
                    answered.store(now(), Ordering::SeqCst);
                    asking.store(false, Ordering::SeqCst);
                });
                // Refused: the app is closing, and there is no one to watch.
                if asked.is_err() {
                    return;
                }
            }

            let silent = Duration::from_millis(now() - answered.load(Ordering::SeqCst));
            if silent >= STALLED && !window_stalled {
                tracing::warn!(
                    target: "tide::ui",
                    "STALLED the window has not answered for {}s",
                    silent.as_secs()
                );
            } else if silent < STALLED && window_stalled {
                tracing::warn!(target: "tide::ui", "RECOVERED the window is answering again");
            }
            window_stalled = silent >= STALLED;

            let behind = app.state::<commands::Terminals>().stalled(STALLED);
            if !behind.is_empty() && !page_stalled {
                tracing::warn!(
                    target: "tide::terminal",
                    "STALLED the page has not drawn the output of {}",
                    behind.join(", ")
                );
            } else if behind.is_empty() && page_stalled {
                tracing::warn!(target: "tide::terminal", "RECOVERED the page is drawing again");
            }
            page_stalled = !behind.is_empty();
        }
    });
}

fn main() {
    start_logging();

    #[cfg(debug_assertions)]
    require_dev_server();

    tracing::info!("starting {}", tide_core::Version);

    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(commands::PendingFiles::from_args())
        .manage(commands::Applied::load())
        .manage(commands::Terminals::default())
        .manage(commands::Remotes::default())
        .manage(commands::Servers::default())
        .manage(commands::Tmuxes::default())
        .manage(commands::Stats::default())
        // However a window goes, its shells and its connection go with it.
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::Destroyed) {
                window
                    .state::<commands::Tmuxes>()
                    .close_window(window.label());
                window
                    .state::<commands::Stats>()
                    .close_window(window.label());
                window
                    .state::<commands::Terminals>()
                    .close_window(window.label());
                window
                    .state::<commands::Remotes>()
                    .close_window(window.label());
                window
                    .state::<commands::Servers>()
                    .close_window(window.label());
            }
        })
        .on_page_load(|webview, payload| {
            // A page starting over has lost its hold on the terminals the last
            // one opened; left alone they would run on with nothing attached.
            if matches!(payload.event(), tauri::webview::PageLoadEvent::Started) {
                webview
                    .state::<commands::Terminals>()
                    .close_window(webview.label());
                webview
                    .state::<commands::Servers>()
                    .close_window(webview.label());
                webview
                    .state::<commands::Tmuxes>()
                    .close_window(webview.label());
                webview
                    .state::<commands::Stats>()
                    .close_window(webview.label());
            }
            tracing::info!(
                target: "tide::ui",
                "PAGE_LOAD event={:?} url={}",
                payload.event(),
                webview.url().map(|u| u.to_string()).unwrap_or_default()
            );
        })
        // Scale and window effects are set on a live window, not declared in
        // the config, so the saved appearance has to be re-applied every launch
        // -- otherwise it survives in settings.json but not on screen.
        .setup(|app| {
            commands::open_main_window(app.handle());
            watch(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ui_ready,
            commands::config,
            commands::set_config,
            commands::set_syntax,
            commands::new_window,
            commands::open_config,
            commands::startup_files,
            commands::read_file,
            commands::write_file,
            commands::file_stamps,
            commands::read_dir,
            commands::dir_stamps,
            commands::real_paths,
            commands::create_file,
            commands::create_dir,
            commands::rename_path,
            commands::delete_path,
            commands::identity,
            commands::git_status,
            commands::remote_connect,
            commands::remote_connect_saved,
            commands::remote_reconnect,
            commands::remote_state,
            commands::remote_disconnect,
            commands::remote_save,
            commands::remote_known,
            commands::remote_forget,
            commands::remote_keys,
            commands::remote_browse,
            commands::session_load,
            commands::session_save,
            commands::terminal_open,
            commands::terminal_write,
            commands::terminal_resize,
            commands::terminal_drawn,
            commands::terminal_close,
            commands::terminal_forget,
            commands::terminal_shells,
            commands::open_external,
            commands::tmux_sessions,
            commands::tmux_open,
            commands::tmux_do,
            commands::tmux_drawn,
            commands::tmux_close,
            commands::sysinfo_start,
            commands::sysinfo_stop,
            commands::remote_signal,
            commands::lsp_catalog,
            commands::lsp_start,
            commands::lsp_send,
            commands::lsp_stop,
            commands::pins,
            commands::add_pin,
            commands::remove_pin,
            commands::move_pin,
            commands::quit_app,
        ])
        .run(tauri::generate_context!())
        .expect("failed to start Tide");
}
