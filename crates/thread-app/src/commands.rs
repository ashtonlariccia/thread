//! Tauri command surface: windows, files, the config, the terminal, and the
//! pinned strip.
//!
//! This layer owns the windows and the IPC transport. Anything that can be
//! decided without a window lives in `thread-core`.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalSize, State, WebviewWindow};

use thread_core::terminal::{self, LocalTerminal, Terminal};
use thread_core::{
    config, document, fsops, tree, Appearance, Config, Document, Entry, Eol, Material, Pin,
    PinStore, Session, Stamp,
};

/// A window's size at 100% scale. Matches the window in tauri.conf.json.
const BASE_SIZE: (f64, f64) = (960.0, 600.0);
const MIN_SIZE: (f64, f64) = (520.0, 360.0);

fn scaled(size: (f64, f64), appearance: &Appearance) -> LogicalSize<f64> {
    let factor = appearance.scale_factor();
    LogicalSize::new(size.0 * factor, size.1 * factor)
}

/// The config, or the defaults if it cannot be read: a config file with a
/// mistake in it should cost the customisation, not the window.
fn stored_config() -> Config {
    config::load().unwrap_or_else(|e| {
        tracing::warn!(target: "thread::ui", "{e}");
        Config::default()
    })
}

fn stored_appearance() -> Appearance {
    stored_config().appearance
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
    build_window(&app, Vec::new())
}

/// Open the config file in a window of its own (Appearance -> Open Config File).
///
/// A new window rather than the current one, so the file can sit beside the
/// window whose look it is changing.
#[tauri::command]
pub async fn open_config(app: AppHandle) -> Result<String, String> {
    let path = config::ensure_file().map_err(|e| e.to_string())?;
    build_window(&app, vec![path.to_string_lossy().into_owned()])
}

/// Build a window, with `files` waiting for it to open once its page is up.
fn build_window(app: &AppHandle, files: Vec<String>) -> Result<String, String> {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(1);

    let label = format!("win-{}", NEXT.fetch_add(1, Ordering::Relaxed));
    // Queued before the window exists, so a page that comes up fast cannot
    // ask for its files before they are there.
    if !files.is_empty() {
        app.state::<PendingFiles>().queue(&label, files);
    }

    let appearance = stored_appearance();
    let size = scaled(BASE_SIZE, &appearance);
    let min = scaled(MIN_SIZE, &appearance);

    let window = tauri::WebviewWindowBuilder::new(app, &label, tauri::WebviewUrl::default())
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

/// Paths a window should open as soon as it is up, by window label.
///
/// Held until the window asks, because a window cannot be told anything
/// before its page has loaded. The main window's are the ones named on the
/// command line (`thread.exe notes.txt`, `thread.exe .`, or "Open with").
pub struct PendingFiles(Mutex<HashMap<String, Vec<String>>>);

impl PendingFiles {
    pub fn from_args() -> Self {
        let files: Vec<String> = std::env::args_os()
            .skip(1)
            // Relative to where the command was run, which the dialogs and the
            // webview will not remember later.
            .filter_map(|arg| std::path::absolute(arg).ok())
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        Self(Mutex::new(HashMap::from([("main".to_owned(), files)])))
    }

    fn queue(&self, label: &str, files: Vec<String>) {
        if let Ok(mut pending) = self.0.lock() {
            pending.entry(label.to_owned()).or_default().extend(files);
        }
    }
}

/// A path waiting for a window, and which of the two things it is.
#[derive(serde::Serialize)]
pub struct StartupPath {
    path: String,
    /// A folder opens in the file tree; anything else opens in the editor.
    dir: bool,
}

/// Hand a window the paths waiting for it, once.
#[tauri::command]
pub fn startup_files(window: WebviewWindow, files: State<'_, PendingFiles>) -> Vec<StartupPath> {
    files
        .0
        .lock()
        .ok()
        .and_then(|mut pending| pending.remove(window.label()))
        .unwrap_or_default()
        .into_iter()
        .map(|path| StartupPath {
            dir: Path::new(&path).is_dir(),
            path,
        })
        .collect()
}

// --- file tree --------------------------------------------------------------

/// One folder's children, for the tree to show when it is unfolded. What it
/// leaves out is the config's `files.exclude`.
#[tauri::command]
pub async fn read_dir(path: String) -> Result<Vec<Entry>, String> {
    let exclude = stored_config().files.exclude;
    tree::list(Path::new(&path), &exclude).map_err(|e| e.to_string())
}

/// When each folder's contents last changed, in order; `None` where it is gone.
///
/// Polled for the folders the tree has unfolded, the same way `file_stamps` is
/// for open files: a `stat` each, and a folder is only re-read once its stamp
/// has moved.
#[tauri::command]
pub async fn dir_stamps(paths: Vec<String>) -> Vec<Option<u64>> {
    paths
        .iter()
        .map(|path| tree::dir_stamp(Path::new(path)))
        .collect()
}

// The tree's file operations. Each hands back the path it made, spelled as the
// listing will spell it, so the frontend can go straight to the new entry.

#[tauri::command]
pub async fn create_file(dir: String, name: String) -> Result<String, String> {
    let path = fsops::create_file(Path::new(&dir), &name).map_err(|e| e.to_string())?;
    tracing::info!(target: "thread::files", "CREATED {}", path.display());
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
pub async fn create_dir(dir: String, name: String) -> Result<String, String> {
    let path = fsops::create_dir(Path::new(&dir), &name).map_err(|e| e.to_string())?;
    tracing::info!(target: "thread::files", "CREATED {}", path.display());
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
pub async fn rename_path(path: String, name: String) -> Result<String, String> {
    let renamed = fsops::rename(Path::new(&path), &name).map_err(|e| e.to_string())?;
    tracing::info!(target: "thread::files", "RENAMED {path} -> {}", renamed.display());
    Ok(renamed.to_string_lossy().into_owned())
}

/// Move a file or folder to the Recycle Bin.
#[tauri::command]
pub async fn delete_path(path: String) -> Result<(), String> {
    fsops::delete(Path::new(&path)).map_err(|e| e.to_string())?;
    tracing::info!(target: "thread::files", "DELETED {path}");
    Ok(())
}

/// Whether `path` is Thread's own config file.
fn is_config_file(path: &Path) -> bool {
    let Ok(config) = config::path() else {
        return false;
    };
    // Canonical on both sides: the same file can be spelled with either slash,
    // any case, or a short 8.3 name.
    match (std::fs::canonicalize(path), std::fs::canonicalize(config)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
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
pub async fn write_file(
    app: AppHandle,
    path: String,
    text: String,
    eol: Eol,
    bom: bool,
) -> Result<Option<Stamp>, String> {
    let stamp = document::write(Path::new(&path), &text, eol, bom).map_err(|e| e.to_string())?;
    tracing::info!(target: "thread::files", "SAVED {path}");

    // Saving the config file is how it is edited, so that is the moment it
    // takes effect -- not the next launch.
    if is_config_file(Path::new(&path)) {
        match config::load() {
            Ok(config) => apply_config(&app, &config),
            // The save itself succeeded; what is in the file just cannot be
            // used. Keep everything as it is and say what is wrong, since the
            // person who can fix it is looking at the file right now.
            Err(e) => {
                tracing::warn!(target: "thread::ui", "config not applied: {e}");
                let _ = app.emit("config-error", e.to_string());
            }
        }
    }
    Ok(stamp)
}

/// The current stamp of each path, in order; `None` where the file is gone.
///
/// The frontend polls this for its open files to notice changes made outside
/// the editor. One `stat` per file, and no file is read unless its stamp moved.
#[tauri::command]
pub async fn file_stamps(paths: Vec<String>) -> Vec<Option<Stamp>> {
    paths
        .iter()
        .map(|path| document::stamp(Path::new(path)))
        .collect()
}

// --- config -----------------------------------------------------------------
//
// Most of the config is the frontend's to act on: it is handed the whole
// thing, and told when it changes. The exception is the zoom and the window
// effect, which only the backend can apply.

/// The config as it stands. Falls back to the defaults if the file has a
/// mistake in it; the mistake itself is reported when the file is saved.
#[tauri::command]
pub fn config() -> Config {
    stored_config()
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

/// The appearance the windows are wearing right now.
///
/// Not the same as what is in the config file: that can be edited by hand,
/// and rescaling a window needs the scale it is *coming from*, which the file
/// no longer says once it has been overwritten.
pub struct Applied(Mutex<Appearance>);

impl Applied {
    pub fn load() -> Self {
        Self(Mutex::new(stored_appearance()))
    }
}

/// Put a config into effect: its appearance onto every window, and the whole
/// of it to their pages.
///
/// Every window rather than the calling one: the config is global, and a
/// second window left opaque while the first went frosted would look like a bug.
fn apply_config(app: &AppHandle, config: &Config) {
    let appearance = &config.appearance;

    // Swapped and released before any window is touched. Window calls made
    // off the main thread wait for it, and the main thread may itself be in
    // here waiting for this lock -- holding it across them would deadlock.
    let previous = {
        let applied = app.state::<Applied>();
        let Ok(mut applied) = applied.0.lock() else {
            return;
        };
        std::mem::replace(&mut *applied, appearance.clone())
    };

    for window in app.webview_windows().values() {
        dress(window, appearance);
        if appearance.scale != previous.scale {
            rescale(window, &previous, appearance);
        }
    }

    // Everything else -- opacity, the editor, the tree, the theme -- is the
    // page's to act on.
    let _ = app.emit("config-changed", config);

    tracing::info!(
        target: "thread::ui",
        "APPEARANCE opacity={} material={:?} scale={}",
        appearance.background_opacity,
        appearance.material,
        appearance.scale,
    );
}

/// Change one setting in the config file and apply it.
///
/// This is what the Appearance dialog does with every control: the same
/// change as editing the key in the file and saving it, made for you.
///
/// Returns the config as it now stands. Values are clamped on the way in, so
/// the dialog must show what came back rather than what it sent.
#[tauri::command]
pub fn set_config(
    app: AppHandle,
    section: String,
    key: String,
    value: serde_json::Value,
) -> Result<Config, String> {
    let config = config::set(&section, &key, &value).map_err(|e| e.to_string())?;
    apply_config(&app, &config);
    Ok(config)
}

// --- session ----------------------------------------------------------------

/// What was open when Thread was last closed.
#[tauri::command]
pub fn session_load() -> Session {
    thread_core::session::load()
}

/// Record what is open now, to come back to. Called as things change rather
/// than at exit, so a crash or a killed process loses nothing either.
#[tauri::command]
pub async fn session_save(session: Session) -> Result<(), String> {
    thread_core::session::save(&session).map_err(|e| e.to_string())
}

// --- terminal ---------------------------------------------------------------
//
// Terminals open as tabs, any number to a window. Each belongs to whatever its
// window is working on: a shell on this machine today, and one on the remote
// host once a window can be connected to one. Everything below deals in
// `dyn Terminal`, so that is a second thing `terminal_open` can make rather
// than a second set of commands.

/// Every open terminal, by id.
#[derive(Default)]
pub struct Terminals(Mutex<HashMap<u64, Open>>);

/// A terminal, with the label of the window it is in.
type Open = (String, Box<dyn Terminal>);

impl Terminals {
    /// Take a terminal out. Dropping what comes back is what ends its shell,
    /// and is left to the caller so that it happens outside the lock.
    fn take(&self, id: u64) -> Option<Box<dyn Terminal>> {
        let (_, terminal) = self.0.lock().ok()?.remove(&id)?;
        Some(terminal)
    }

    /// The window has gone, or its page has been reloaded and can no longer
    /// reach them: its shells go too.
    pub fn close_window(&self, label: &str) {
        let Ok(mut open) = self.0.lock() else {
            return;
        };
        let ids: Vec<u64> = open
            .iter()
            .filter(|(_, (window, _))| window == label)
            .map(|(id, _)| *id)
            .collect();
        let closed: Vec<_> = ids.iter().filter_map(|id| open.remove(id)).collect();
        drop(open);
        drop(closed);
    }

    fn with(&self, id: u64, act: impl FnOnce(&dyn Terminal)) {
        if let Ok(open) = self.0.lock() {
            if let Some((_, terminal)) = open.get(&id) {
                act(terminal.as_ref());
            }
        }
    }
}

/// Start a terminal for this window, in `cwd`. Output arrives on `on_output`
/// as raw bytes; `on_exit` hears once, when the shell has ended.
#[tauri::command]
pub async fn terminal_open(
    app: AppHandle,
    window: WebviewWindow,
    on_output: Channel<InvokeResponseBody>,
    on_exit: Channel<()>,
    cwd: Option<String>,
    cols: u16,
    rows: u16,
) -> Result<u64, String> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);

    let label = window.label().to_owned();
    let id = NEXT.fetch_add(1, Ordering::Relaxed);

    let options = terminal::Options {
        shell: stored_config().terminal.shell,
        cwd: cwd.map(Into::into),
        cols,
        rows,
    };
    let spawned = LocalTerminal::spawn(
        options,
        move |chunk| {
            // A channel that refuses is a page that has gone.
            let _ = on_output.send(InvokeResponseBody::Raw(chunk.to_vec()));
        },
        {
            let app = app.clone();
            move || {
                drop(app.state::<Terminals>().take(id));
                let _ = on_exit.send(());
                tracing::info!(target: "thread::terminal", "ENDED #{id}");
            }
        },
    )
    .map_err(|e| e.to_string())?;

    tracing::info!(target: "thread::terminal", "OPENED #{id} in {label} {cols}x{rows}");
    if let Ok(mut open) = app.state::<Terminals>().0.lock() {
        open.insert(id, (label, Box::new(spawned)));
    }
    Ok(id)
}

// Not `async`, unlike the rest: these run in the order they were sent, which
// is the order the keys were pressed. Neither waits on the shell.

/// What was typed, or pasted, as the terminal emulator encoded it.
#[tauri::command]
pub fn terminal_write(terminals: State<'_, Terminals>, id: u64, data: String) {
    terminals.with(id, |terminal| terminal.write(data.as_bytes()));
}

#[tauri::command]
pub fn terminal_resize(terminals: State<'_, Terminals>, id: u64, cols: u16, rows: u16) {
    terminals.with(id, |terminal| terminal.resize(cols, rows));
}

/// End a terminal and everything running in it. Nothing happens if it has
/// already gone.
#[tauri::command]
pub fn terminal_close(terminals: State<'_, Terminals>, id: u64) {
    drop(terminals.take(id));
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
