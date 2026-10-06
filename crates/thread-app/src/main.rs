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
    tracing::info!(target: "thread::ui", "UI_READY {detail}");
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
    message_box("Thread - dev server not running", message);
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

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("THREAD_LOG")
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    #[cfg(debug_assertions)]
    require_dev_server();

    tracing::info!("starting {}", thread_core::Version);

    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(commands::PendingFiles::from_args())
        .manage(commands::Applied::load())
        .manage(commands::Terminals::default())
        .manage(commands::Remotes::default())
        .manage(commands::Servers::default())
        // However a window goes, its shells and its connection go with it.
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::Destroyed) {
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
            }
            tracing::info!(
                target: "thread::ui",
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
            commands::create_file,
            commands::create_dir,
            commands::rename_path,
            commands::delete_path,
            commands::identity,
            commands::git_branch,
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
            commands::terminal_close,
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
        .expect("failed to start Thread");
}
