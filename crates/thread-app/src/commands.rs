//! Tauri command surface: windows, files, appearance, and the pinned strip.
//!
//! This layer owns the windows and the IPC transport. Anything that can be
//! decided without a window lives in `thread-core`.

use std::path::Path;
use std::sync::Mutex;
use tauri::{AppHandle, LogicalSize, Manager, PhysicalSize, State, WebviewWindow};

use thread_core::{document, settings, Appearance, Document, Eol, Material, Pin, PinStore};

/// A window's size at 100% scale. Matches the window in tauri.conf.json.
const BASE_SIZE: (f64, f64) = (960.0, 600.0);
const MIN_SIZE: (f64, f64) = (520.0, 360.0);

fn scaled(size: (f64, f64), appearance: &Appearance) -> LogicalSize<f64> {
    let factor = appearance.scale_factor();
    LogicalSize::new(size.0 * factor, size.1 * factor)
}

/// The stored appearance, or the defaults if it cannot be read: an unreadable
/// settings file should cost the customisation, not the window.
fn stored_appearance() -> Appearance {
    settings::load().unwrap_or_else(|e| {
        tracing::warn!(target: "thread::ui", "could not read settings: {e}");
        Appearance::default()
    })
}

// --- windows ----------------------------------------------------------------

/// Apply the parts of the appearance that belong to the OS window and the
/// webview rather than to the page: the zoom and the material.
///
/// Acrylic is a property of the window, not of the page inside it, so it cannot
/// be done from CSS. Both are per-window and do not carry into a new one.
fn dress(window: &WebviewWindow, appearance: &Appearance) {
    use tauri::window::{Effect, EffectsBuilder};

    let label = window.label();

    if let Err(e) = window.set_zoom(appearance.scale_factor()) {
        tracing::warn!(target: "thread::ui", "set_zoom on {label} failed: {e}");
    }

    let effects = match appearance.material {
        Material::Acrylic => Some(EffectsBuilder::new().effect(Effect::Acrylic).build()),
        // `None` clears whatever was applied before, so turning the toggle off
        // actually removes the blur rather than leaving it stuck on.
        Material::None => None,
    };
    if let Err(e) = window.set_effects(effects) {
        // Not fatal: the opacity half still works, and an unsupported material
        // should cost the blur, not the settings dialog.
        tracing::warn!(target: "thread::ui", "set_effects on {label} failed: {e}");
    }
}

/// Size, dress and reveal the window declared in tauri.conf.json.
///
/// It is declared hidden so the stored scale can be applied first; shown
/// straight away it would open at 100% and visibly jump.
pub fn open_main_window(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let appearance = stored_appearance();

    // Minimum first: at a scale below 100% the configured minimum would
    // otherwise refuse the smaller size.
    let _ = window.set_min_size(Some(scaled(MIN_SIZE, &appearance)));
    let _ = window.set_size(scaled(BASE_SIZE, &appearance));
    dress(&window, &appearance);

    if let Err(e) = window.show() {
        tracing::error!(target: "thread::ui", "could not show the main window: {e}");
    }
}

/// Open another Thread window (File -> New Window).
///
/// A second webview in the *same* process rather than a second process: it
/// shares the WebView2 host, which keeps the memory cost of an extra window
/// small. Labels are `win-N`, matched by the `win-*` entry in
/// `capabilities/default.json` -- without that the new window would have no
/// permissions and its menus would silently fail.
///
/// **Must be `async`.** Synchronous Tauri commands run on the main thread, and
/// window creation needs that thread to pump the event loop — so building a
/// window from a sync command deadlocks: `build()` simply never returns, and the
/// half-created window shows as a blank white pane. Marking it async moves the
/// work off the main thread.
#[tauri::command]
pub async fn new_window(app: AppHandle) -> Result<String, String> {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(1);

    let label = format!("win-{}", NEXT.fetch_add(1, Ordering::Relaxed));
    let appearance = stored_appearance();
    let size = scaled(BASE_SIZE, &appearance);
    let min = scaled(MIN_SIZE, &appearance);

    let window = tauri::WebviewWindowBuilder::new(&app, &label, tauri::WebviewUrl::default())
        .title("Thread")
        .inner_size(size.width, size.height)
        .min_inner_size(min.width, min.height)
        .decorations(false)
        // Matches the window in tauri.conf.json. The page paints its own
        // background, so an opaque look costs nothing -- but a window built
        // opaque could never become translucent without a restart.
        .transparent(true)
        .build()
        .map_err(|e| format!("could not open a new window: {e}"))?;

    dress(&window, &appearance);

    tracing::info!("opened window {label}");
    Ok(label)
}

/// Quit the application (File -> Exit).
///
/// By closing every window rather than exiting the process: each window then
/// gets to ask about its own unsaved files, and one that is told "Cancel"
/// stays open. The app exits by itself once the last window has gone.
#[tauri::command]
pub fn quit_app(app: AppHandle) {
    tracing::info!("closing every window on user request");
    for window in app.webview_windows().values() {
        let _ = window.close();
    }
}

// --- files ------------------------------------------------------------------
//
// `async` so the disk is never touched on the main thread: a slow drive or a
// large file must cost the open, not the window's event loop.

/// Files named on the command line (`thread.exe notes.txt`, or "Open with").
///
/// Held until a window asks, because the window that should open them does
/// not exist yet when the process starts.
pub struct StartupFiles(Mutex<Vec<String>>);

impl StartupFiles {
    pub fn from_args() -> Self {
        let files = std::env::args_os()
            .skip(1)
            // Relative to where the command was run, which the dialogs and the
            // webview will not remember later.
            .filter_map(|arg| std::path::absolute(arg).ok())
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        Self(Mutex::new(files))
    }
}

/// Hand the command-line files to the first window that asks, once.
#[tauri::command]
pub fn startup_files(files: State<'_, StartupFiles>) -> Vec<String> {
    files
        .0
        .lock()
        .map(|mut files| std::mem::take(&mut *files))
        .unwrap_or_default()
}

#[tauri::command]
pub async fn read_file(path: String) -> Result<Document, String> {
    let doc = document::read(Path::new(&path)).map_err(|e| e.to_string())?;
    tracing::info!(target: "thread::files", "OPENED {path} ({} bytes)", doc.text.len());
    Ok(doc)
}

/// `text` is `\n`-separated; `eol` and `bom` are what the file had when it was
/// opened, and are restored here.
#[tauri::command]
pub async fn write_file(path: String, text: String, eol: Eol, bom: bool) -> Result<(), String> {
    document::write(Path::new(&path), &text, eol, bom).map_err(|e| e.to_string())?;
    tracing::info!(target: "thread::files", "SAVED {path}");
    Ok(())
}

// --- appearance -------------------------------------------------------------
//
// Two halves: the values the frontend reads to drive the CSS, and the zoom and
// window effect, which only the backend can apply.

#[tauri::command]
pub fn appearance() -> Result<Appearance, String> {
    settings::load().map_err(|e| e.to_string())
}

/// Resize a window so it shows the same layout at the new scale.
///
/// Relative to its current size rather than back to the base size: a window
/// that was dragged wide stays proportionally wide.
fn rescale(window: &WebviewWindow, from: &Appearance, to: &Appearance) {
    // Minimum first, so shrinking is not refused by the old, larger minimum.
    let _ = window.set_min_size(Some(scaled(MIN_SIZE, to)));

    // A maximised window already fills its monitor; only the zoom changes.
    if window.is_maximized().unwrap_or(false) {
        return;
    }

    let Ok(size) = window.inner_size() else {
        return;
    };
    let ratio = to.scale_factor() / from.scale_factor();
    let _ = window.set_size(PhysicalSize::new(
        (f64::from(size.width) * ratio).round() as u32,
        (f64::from(size.height) * ratio).round() as u32,
    ));
}

/// Store the appearance and apply the parts the backend owns.
///
/// Applied to every window rather than the calling one: the appearance is
/// global, and a second window left opaque while the first went frosted would
/// look like a bug.
///
/// Returns what was actually written: the store clamps opacity and scale, so
/// the dialog must render the stored value rather than the one it sent.
#[tauri::command]
pub fn set_appearance(app: AppHandle, appearance: Appearance) -> Result<Appearance, String> {
    let previous = stored_appearance();
    let stored = settings::save(&appearance).map_err(|e| e.to_string())?;

    for window in app.webview_windows().values() {
        dress(window, &stored);
        if stored.scale != previous.scale {
            rescale(window, &previous, &stored);
        }
    }

    tracing::info!(
        target: "thread::ui",
        "APPEARANCE opacity={} material={:?} scale={}",
        stored.background_opacity,
        stored.material,
        stored.scale,
    );
    Ok(stored)
}

// --- pins -------------------------------------------------------------------
//
// The strip along the bottom of the window. Every command returns the whole
// strip, so the UI never has to guess what changed.

#[tauri::command]
pub fn pins() -> Result<Vec<Pin>, String> {
    Ok(PinStore::load().map_err(|e| e.to_string())?.list())
}

/// Pinning something already pinned is not an error — it refreshes the stored
/// label and leaves the strip's length alone.
#[tauri::command]
pub fn add_pin(pin: Pin) -> Result<Vec<Pin>, String> {
    let mut store = PinStore::load().map_err(|e| e.to_string())?;
    let added = store.add(pin.clone());
    store.save().map_err(|e| e.to_string())?;

    if added {
        tracing::info!(target: "thread::pins", "PINNED {} {}", pin.kind, pin.target);
    }
    Ok(store.list())
}

/// Drop a pin at a new position on the strip (drag to reorder).
///
/// `index` counts in the list as it will be *after* the move, which is what the
/// drag already knows: the gap it was released over.
#[tauri::command]
pub fn move_pin(kind: String, target: String, index: usize) -> Result<Vec<Pin>, String> {
    let mut store = PinStore::load().map_err(|e| e.to_string())?;
    if store.move_to(&kind, &target, index) {
        store.save().map_err(|e| e.to_string())?;
        tracing::info!(target: "thread::pins", "PIN_MOVED {kind} {target} -> {index}");
    }
    Ok(store.list())
}

#[tauri::command]
pub fn remove_pin(kind: String, target: String) -> Result<Vec<Pin>, String> {
    let mut store = PinStore::load().map_err(|e| e.to_string())?;
    if store.remove(&kind, &target) {
        store.save().map_err(|e| e.to_string())?;
        tracing::info!(target: "thread::pins", "UNPINNED {kind} {target}");
    }
    Ok(store.list())
}
