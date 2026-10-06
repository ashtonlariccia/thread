//! The config file, `%APPDATA%\thread\config.toml`.
//!
//! One hand-editable file for everything that is a preference. It is read, not
//! owned: Thread never rewrites it wholesale, so comments and layout survive.
//! The one thing that writes to it — the Appearance dialog — patches the single
//! key being changed and leaves every other byte alone. The dialog and the file
//! are two views of the same settings, and either can be used.
//!
//! Every key has a default, so an absent or partial file is not an error and a
//! key added here does not invalidate an existing file. A key that is not
//! recognised is ignored rather than refused, so a config written for a newer
//! build still loads in an older one.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use toml_edit::{DocumentMut, Item};

use crate::{data_dir, strip_bom, Error, Result};

const FILE: &str = "config.toml";
/// What the config was before it was TOML. Read once, to carry it over.
const LEGACY_FILE: &str = "settings.json";

/// The file written when there is none: every key, at its default, with a note
/// on what it does. Parsing this must give exactly [`Config::default`] — a
/// test holds the two together.
pub const TEMPLATE: &str = r##"# Thread configuration.
#
# Saved changes apply immediately. Any key left out takes the default shown.

[appearance]
# How solid the window is, in percent: 20 to 100.
background_opacity = 100
# What is behind a translucent window: "none", or "acrylic" to frost it.
material = "none"
# Zooms the whole interface and sizes the window to match, in percent: 50 to 200.
scale = 100

[editor]
# A CSS font list: the first one installed is used.
font_family = "'Operator Mono', 'Geist Mono', Consolas, monospace"
font_size = 14
# Line height, as a multiple of the font size.
line_height = 1.6
# How wide a tab is, and how far Tab indents.
tab_width = 4
# Indent with spaces (true) or tab characters (false).
insert_spaces = true
# Work out tabs-or-spaces and the width from each file when it is opened,
# and use that in preference to the two settings above.
detect_indentation = true
line_numbers = true
# Number lines by their distance from the cursor, with the cursor's own line
# showing its real number.
relative_line_numbers = false
word_wrap = false
# Glide the caret to where it is going instead of jumping there.
smooth_caret = true
# Type the closing bracket or quote along with the opening one.
auto_close = true

[files]
# Names the file tree leaves out. `*` is any run of characters and `?` is any
# one, matched against the name alone, so ".*" is every dotfile at any depth.
exclude = [".*"]

[theme]
# Colours for syntax highlighting. Available: "catppuccin".
syntax = "catppuccin"
# The colour things are highlighted with: the active tab, the selected row,
# a focused field, the buttons that go ahead. Six hex digits.
accent = "#cba6f7"

[vim]
# Vim motions in the editor: modes, operators, registers, macros and the
# `:` command line. `:w`, `:q`, `:wq`, `:e`, `:bn` and `:bp` act on Thread's
# files, and `:sp`, `:vsp`, `:new`, `:vnew`, `:close` and `:only` on its panes.
enabled = true

[terminal]
# The command line the terminal runs, such as "cmd.exe" or "wsl.exe". Empty
# picks PowerShell 7 if it is installed, and Windows PowerShell if not.
shell = ""
# The size of the text, in pixels. The font itself is the editor's.
font_size = 14
# The cursor's shape: "block", "bar" or "underline".
cursor = "block"
# How many lines that have scrolled off the top are kept to scroll back to.
scrollback = 2000

[lsp]
# The language servers that are switched on, by name: completion as you type,
# and what is wrong with a line written at the end of it. A server has to be
# installed to run; Edit -> LSPs lists the ones Thread knows, says which are,
# and switches them on and off. Such as: ["rust-analyzer", "typescript"].
enabled = []

# Per-language overrides for `tab_width` and `insert_spaces`. The name is the
# one shown in the bottom bar, in lower case. Detected indentation still wins
# while `detect_indentation` is on.
#
# [language.makefile]
# insert_spaces = false
#
# [language.yaml]
# tab_width = 2

# Languages of your own: highlighting for file extensions Thread has no
# grammar for. Each is a table named for the language, and is what
# Edit -> LSPs -> Your Languages writes. Every key but `extensions` is optional.
#
# [syntax.Mylang]
# extensions = ["my", "myl"]
# line_comment = "#"
# block_comment = ["/*", "*/"]
# strings = ['"', "'"]
# keywords = ["if", "else", "while", "return"]
# types = ["int", "string"]
# constants = ["true", "false", "nil"]
# functions = ["print", "len"]
# # Anything the lists cannot say, as regular expressions tried at the start
# # of each token. `as` is one of: keyword, type, constant, function, builtin,
# # operator, string, comment, number, property.
# patterns = [{ match = '@[a-z]+', as = "builtin" }]
"##;

/// The material drawn behind a translucent window.
///
/// `None` is a plain translucent window with nothing behind it; `Acrylic` is
/// the frosted blur Windows Terminal uses. Mica is deliberately absent: it
/// tints from the desktop wallpaper and ignores opacity, which makes it fight
/// the opacity setting rather than compose with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Material {
    #[default]
    None,
    Acrylic,
}

/// What the window looks like.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    /// Percent. 100 is fully opaque; the window only goes translucent below it.
    pub background_opacity: u8,
    pub material: Material,
    /// Percent. Zooms the interface and sizes the window to match, so 150 is
    /// the same layout half as big again rather than more room at the same size.
    pub scale: u16,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            background_opacity: 100,
            material: Material::default(),
            scale: 100,
        }
    }
}

impl Appearance {
    /// Clamp anything a hand-edited file could get wrong.
    ///
    /// A fully transparent background makes the window unclickable-looking,
    /// and a scale near zero leaves nothing to click — both are states a user
    /// could not get out of from inside the app.
    pub fn sanitised(mut self) -> Self {
        self.background_opacity = self.background_opacity.clamp(20, 100);
        self.scale = self.scale.clamp(50, 200);
        self
    }

    /// The scale as a multiplier, as the webview and the window want it.
    pub fn scale_factor(&self) -> f64 {
        f64::from(self.scale) / 100.0
    }
}

/// How text is shown and typed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Editor {
    /// A CSS font list, passed through as written.
    pub font_family: String,
    /// Pixels, before the interface scale.
    pub font_size: f64,
    /// A multiple of the font size.
    pub line_height: f64,
    pub tab_width: u8,
    pub insert_spaces: bool,
    pub detect_indentation: bool,
    pub line_numbers: bool,
    /// Counted from the cursor's line. Only applies while `line_numbers` is on.
    pub relative_line_numbers: bool,
    pub word_wrap: bool,
    /// Animate the caret between positions.
    pub smooth_caret: bool,
    /// Close brackets and quotes as they are opened.
    pub auto_close: bool,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            font_family: "'Operator Mono', 'Geist Mono', Consolas, monospace".into(),
            font_size: 14.0,
            line_height: 1.6,
            tab_width: 4,
            insert_spaces: true,
            detect_indentation: true,
            line_numbers: true,
            relative_line_numbers: false,
            word_wrap: false,
            smooth_caret: true,
            auto_close: true,
        }
    }
}

impl Editor {
    /// Text nobody can read, or a tab a screen wide, is not a preference.
    fn sanitised(mut self) -> Self {
        let defaults = Self::default();
        if self.font_family.trim().is_empty() {
            self.font_family = defaults.font_family;
        }
        // `clamp` passes NaN through, and a NaN font size renders nothing.
        self.font_size = if self.font_size.is_finite() {
            self.font_size.clamp(6.0, 72.0)
        } else {
            defaults.font_size
        };
        self.line_height = if self.line_height.is_finite() {
            self.line_height.clamp(1.0, 3.0)
        } else {
            defaults.line_height
        };
        self.tab_width = self.tab_width.clamp(1, 16);
        self
    }
}

/// What the file tree shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Files {
    /// File and folder names left out, as globs over the name (see
    /// [`crate::tree::matches`]).
    pub exclude: Vec<String>,
}

impl Default for Files {
    fn default() -> Self {
        Self {
            exclude: vec![".*".into()],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Theme {
    /// The name of the syntax-highlighting palette. The palettes themselves
    /// belong to the frontend; a name it does not know falls back to the default.
    pub syntax: String,
    /// What the window highlights with, as `#rrggbb`.
    pub accent: String,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            syntax: "catppuccin".into(),
            accent: "#cba6f7".into(),
        }
    }
}

impl Theme {
    /// Something that is not a colour would leave the window with no
    /// highlight at all, which is not a look anyone chose.
    fn sanitised(mut self) -> Self {
        let digits = self.accent.strip_prefix('#').unwrap_or("");
        if digits.len() == 6 && digits.bytes().all(|b| b.is_ascii_hexdigit()) {
            self.accent = self.accent.to_ascii_lowercase();
        } else {
            self.accent = Self::default().accent;
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Vim {
    pub enabled: bool,
}

impl Default for Vim {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// The shape of a terminal's cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TerminalCursor {
    #[default]
    Block,
    Bar,
    Underline,
}

/// The terminals that open as tabs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Terminal {
    /// A command line, run as written. Empty leaves the choice to
    /// [`crate::terminal::default_shell`].
    pub shell: String,
    /// Lines kept above the screen. Every one is held in memory.
    pub scrollback: u32,
    /// Pixels, before the interface scale. Its own, not the editor's: what
    /// reads well as prose to edit and as output to scan are not the same.
    pub font_size: f64,
    pub cursor: TerminalCursor,
}

impl Default for Terminal {
    fn default() -> Self {
        Self {
            shell: String::new(),
            scrollback: 2000,
            font_size: 14.0,
            cursor: TerminalCursor::default(),
        }
    }
}

impl Terminal {
    fn sanitised(mut self) -> Self {
        // Scrollback is memory; a slip of the hand should not cost a gigabyte.
        self.scrollback = self.scrollback.min(100_000);
        // `clamp` passes NaN through, and text of no size cannot be read.
        self.font_size = if self.font_size.is_finite() {
            self.font_size.clamp(6.0, 72.0)
        } else {
            Self::default().font_size
        };
        self
    }
}

/// The language servers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Lsp {
    /// The ids of the servers that are switched on (see [`crate::lsp::CATALOG`]).
    /// One that is not installed, or not known, is simply not run.
    pub enabled: Vec<String>,
}

/// A language defined in the config: how files with its extensions are
/// highlighted. The frontend builds the tokeniser; this is only what it is
/// built from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Syntax {
    /// Without the dot, lower-case.
    pub extensions: Vec<String>,
    /// What starts a comment that runs to the end of the line; empty for none.
    pub line_comment: String,
    /// What opens and what closes a comment that can span lines: two, or none.
    pub block_comment: Vec<String>,
    /// The delimiters a string starts and ends with.
    pub strings: Vec<String>,
    pub keywords: Vec<String>,
    pub types: Vec<String>,
    pub constants: Vec<String>,
    pub functions: Vec<String>,
    pub patterns: Vec<SyntaxPattern>,
}

/// A regular expression, and what to colour its matches as.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SyntaxPattern {
    #[serde(rename = "match")]
    pub pattern: String,
    #[serde(rename = "as")]
    pub category: String,
}

impl Syntax {
    fn sanitised(mut self) -> Self {
        let words = |list: Vec<String>| {
            let mut seen = Vec::new();
            for word in list {
                let word = word.trim().to_owned();
                if !word.is_empty() && !seen.contains(&word) {
                    seen.push(word);
                }
            }
            seen
        };
        self.extensions = words(
            self.extensions
                .into_iter()
                .map(|e| e.trim().trim_start_matches('.').to_lowercase())
                .collect(),
        );
        self.line_comment = self.line_comment.trim().to_owned();
        // Half a pair opens a comment nothing can close.
        self.block_comment = words(self.block_comment);
        if self.block_comment.len() != 2 {
            self.block_comment.clear();
        }
        self.strings = words(self.strings);
        self.keywords = words(self.keywords);
        self.types = words(self.types);
        self.constants = words(self.constants);
        self.functions = words(self.functions);
        self.patterns.retain(|p| !p.pattern.is_empty());
        self
    }
}

/// What one language does differently from `[editor]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct LanguageOverride {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab_width: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub insert_spaces: Option<bool>,
}

/// Everything in the config file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub appearance: Appearance,
    pub editor: Editor,
    pub files: Files,
    pub theme: Theme,
    pub vim: Vim,
    pub terminal: Terminal,
    pub lsp: Lsp,
    /// By language name, lower-case.
    pub language: BTreeMap<String, LanguageOverride>,
    /// Languages of the user's own, by the name the bottom bar shows.
    pub syntax: BTreeMap<String, Syntax>,
}

impl Config {
    fn sanitised(mut self) -> Self {
        self.appearance = self.appearance.sanitised();
        self.editor = self.editor.sanitised();
        self.theme = self.theme.sanitised();
        self.terminal = self.terminal.sanitised();
        // Keyed in lower case, so `[language.Rust]` and `[language.rust]` are
        // the same thing to whoever looks one up.
        self.language = self
            .language
            .into_iter()
            .map(|(name, mut over)| {
                over.tab_width = over.tab_width.map(|w| w.clamp(1, 16));
                (name.to_lowercase(), over)
            })
            .collect();
        self.syntax = self
            .syntax
            .into_iter()
            .map(|(name, syntax)| (name.trim().to_owned(), syntax.sanitised()))
            .filter(|(name, _)| !name.is_empty())
            .collect();
        self
    }
}

/// Parse config text. The error says where in the file the problem is.
pub fn parse(text: &str) -> Result<Config> {
    toml::from_str::<Config>(strip_bom(text))
        .map(Config::sanitised)
        .map_err(|e| Error::Other(anyhow::anyhow!("{}", e.to_string().trim_end())))
}

/// Where the config file lives, whether or not it exists yet.
pub fn path() -> Result<PathBuf> {
    Ok(data_dir()?.join(FILE))
}

/// The config, or the defaults if there is no file.
///
/// A file that does not parse is an error rather than a silent fallback: the
/// caller decides whether to carry on with defaults, and can say why.
pub fn load() -> Result<Config> {
    let path = path()?;
    match std::fs::read_to_string(&path) {
        Ok(text) => parse(&text).map_err(|e| {
            Error::Other(anyhow::anyhow!(
                "{} has a mistake in it:\n{e}",
                path.display()
            ))
        }),
        // No file yet is the normal first run, not a failure.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(legacy().unwrap_or_default()),
        Err(e) => Err(Error::Io(e)),
    }
}

/// The config file's path, having written one if there was none.
///
/// For opening it in an editor: an empty buffer would show none of the keys
/// there are to set. An existing file is left exactly as it is.
pub fn ensure_file() -> Result<PathBuf> {
    let path = path()?;
    if !path.exists() {
        // Whatever was set under the old format comes along.
        let config = legacy().unwrap_or_default();
        std::fs::write(&path, render(&config))?;
    }
    Ok(path)
}

/// Change one setting, leaving the rest of the file — other keys, comments,
/// layout — as it is. Returns the config as it now stands.
///
/// `value` arrives as JSON because that is what the frontend speaks; it is
/// written as the TOML of the same shape.
pub fn set(section: &str, key: &str, value: &serde_json::Value) -> Result<Config> {
    let path = ensure_file()?;
    let text = std::fs::read_to_string(&path)?;
    let (text, config) = patched(strip_bom(&text), section, key, value)?;
    std::fs::write(&path, text)?;
    Ok(config)
}

/// Write, replace or remove a language of the user's own, leaving the rest of
/// the file as it is. `previous` is the name it had, if it has been renamed;
/// `syntax` of `None` removes it. Returns the config as it now stands.
pub fn set_syntax(name: &str, previous: Option<&str>, syntax: Option<&Syntax>) -> Result<Config> {
    let path = ensure_file()?;
    let text = std::fs::read_to_string(&path)?;
    let (text, config) = with_syntax(strip_bom(&text), name, previous, syntax)?;
    std::fs::write(&path, text)?;
    Ok(config)
}

fn with_syntax(
    text: &str,
    name: &str,
    previous: Option<&str>,
    syntax: Option<&Syntax>,
) -> Result<(String, Config)> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::Other(anyhow::anyhow!("a language needs a name")));
    }

    let mut doc = text.parse::<DocumentMut>().unwrap_or_else(|_| template());
    let mut made = syntax.map(|syntax| syntax_table(&syntax.clone().sanitised()));
    // What the file ends with -- in a new one, the notes on the tables that
    // can be added -- stays above a table added below it, rather than being
    // left to read as though it were about the table above.
    if let Some(made) = &mut made {
        if let Some(trailing) = doc.trailing().as_str().filter(|t| !t.trim().is_empty()) {
            made.decor_mut()
                .set_prefix(format!("{}\n", trailing.trim_end()));
            doc.set_trailing("");
        }
    }
    let languages = table(&mut doc, "syntax");
    // Only its languages are written, as `[syntax.Name]`; a bare `[syntax]`
    // above them would be a heading with nothing under it.
    languages.set_implicit(true);
    if let Some(previous) = previous {
        languages.remove(previous);
    }
    languages.remove(name);
    if let Some(made) = made {
        languages.insert(name, Item::Table(made));
    }

    let text = doc.to_string();
    let config = parse(&text)?;
    Ok((text, config))
}

/// A language as a TOML table. What it does not set is left out, so the file
/// says what the language has rather than everything it could have.
fn syntax_table(syntax: &Syntax) -> toml_edit::Table {
    let list = |words: &[String]| {
        let mut array = toml_edit::Array::new();
        array.extend(words.iter().map(String::as_str));
        toml_edit::value(array)
    };

    let mut table = toml_edit::Table::new();
    table["extensions"] = list(&syntax.extensions);
    if !syntax.line_comment.is_empty() {
        table["line_comment"] = toml_edit::value(syntax.line_comment.as_str());
    }
    for (key, words) in [
        ("block_comment", &syntax.block_comment),
        ("strings", &syntax.strings),
        ("keywords", &syntax.keywords),
        ("types", &syntax.types),
        ("constants", &syntax.constants),
        ("functions", &syntax.functions),
    ] {
        if !words.is_empty() {
            table[key] = list(words);
        }
    }
    if !syntax.patterns.is_empty() {
        let mut patterns = toml_edit::Array::new();
        for pattern in &syntax.patterns {
            let mut entry = toml_edit::InlineTable::new();
            entry.insert("match", pattern.pattern.as_str().into());
            entry.insert("as", pattern.category.as_str().into());
            patterns.push(entry);
        }
        table["patterns"] = toml_edit::value(patterns);
    }
    table
}

/// `text` with one setting changed, and the config that results.
fn patched(
    text: &str,
    section: &str,
    key: &str,
    value: &serde_json::Value,
) -> Result<(String, Config)> {
    let refuse = |why: String| {
        Error::Other(anyhow::anyhow!(
            "{section}.{key} cannot be set to {value}: {why}"
        ))
    };

    // A file that will not parse is replaced: the dialog has to be able to dig
    // the app out of a bad config.
    let mut doc = text.parse::<DocumentMut>().unwrap_or_else(|_| template());
    set_value(&mut doc, section, key, to_toml(value).map_err(refuse)?);
    let config = parse(&doc.to_string()).map_err(|e| refuse(e.to_string()))?;

    // What the setting came out as once it was read back: clamped, perhaps,
    // or not there at all if this is not a setting the config has.
    let stored = serde_json::to_value(&config)
        .ok()
        .and_then(|all| all.get(section)?.get(key).cloned())
        .ok_or_else(|| refuse("there is no such setting".into()))?;

    // The file should say what is in effect, so a value that was clamped is
    // written as clamped. Compared as numbers where they are numbers: 14 and
    // 14.0 are the same setting, and not worth rewriting the line over.
    let same = match (stored.as_f64(), value.as_f64()) {
        (Some(a), Some(b)) => a == b,
        _ => &stored == value,
    };
    if !same {
        set_value(&mut doc, section, key, to_toml(&stored).map_err(refuse)?);
    }
    Ok((doc.to_string(), config))
}

fn to_toml(value: &serde_json::Value) -> std::result::Result<toml_edit::Value, String> {
    use serde_json::Value as Json;
    match value {
        Json::Bool(b) => Ok((*b).into()),
        Json::String(s) => Ok(s.as_str().into()),
        Json::Number(n) => n
            .as_i64()
            .map(toml_edit::Value::from)
            .or_else(|| n.as_f64().map(toml_edit::Value::from))
            .ok_or_else(|| "the number is out of range".to_string()),
        Json::Array(items) => {
            let mut array = toml_edit::Array::new();
            for item in items {
                array.push(to_toml(item)?);
            }
            Ok(array.into())
        }
        Json::Null | Json::Object(_) => Err("that kind of value cannot be a setting".into()),
    }
}

fn template() -> DocumentMut {
    TEMPLATE.parse().expect("the config template is valid TOML")
}

/// The template, with the settings the old format could hold filled in.
fn render(config: &Config) -> String {
    let mut doc = template();
    let appearance = &config.appearance;
    set_value(
        &mut doc,
        "appearance",
        "background_opacity",
        i64::from(appearance.background_opacity).into(),
    );
    set_value(
        &mut doc,
        "appearance",
        "material",
        match appearance.material {
            Material::None => "none",
            Material::Acrylic => "acrylic",
        }
        .into(),
    );
    set_value(
        &mut doc,
        "appearance",
        "scale",
        i64::from(appearance.scale).into(),
    );

    let mut exclude = toml_edit::Array::new();
    exclude.extend(config.files.exclude.iter().map(String::as_str));
    set_value(&mut doc, "files", "exclude", exclude.into());

    doc.to_string()
}

/// The named top-level table, created — or put right, if something that is not
/// a table is sitting there — as needed.
fn table<'a>(doc: &'a mut DocumentMut, name: &str) -> &'a mut toml_edit::Table {
    let item = doc.entry(name).or_insert_with(toml_edit::table);
    if !item.is_table() {
        *item = toml_edit::table();
    }
    match item {
        Item::Table(table) => table,
        _ => unreachable!("just made it a table"),
    }
}

fn set_value(doc: &mut DocumentMut, section: &str, key: &str, value: toml_edit::Value) {
    let section = table(doc, section);
    // Assigned through the existing value where there is one, so a comment on
    // the same line as a key stays attached to it.
    match section.get_mut(key) {
        Some(Item::Value(existing)) => {
            let decor = existing.decor().clone();
            *existing = value;
            *existing.decor_mut() = decor;
        }
        _ => section[key] = Item::Value(value),
    }
}

/// What `settings.json` held, if there is one to read.
fn legacy() -> Option<Config> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Legacy {
        background_opacity: Option<u8>,
        material: Option<Material>,
        scale: Option<u16>,
        exclude: Option<Vec<String>>,
    }

    let text = std::fs::read_to_string(data_dir().ok()?.join(LEGACY_FILE)).ok()?;
    let old: Legacy = serde_json::from_str(strip_bom(&text)).ok()?;

    let mut config = Config::default();
    if let Some(opacity) = old.background_opacity {
        config.appearance.background_opacity = opacity;
    }
    if let Some(material) = old.material {
        config.appearance.material = material;
    }
    if let Some(scale) = old.scale {
        config.appearance.scale = scale;
    }
    if let Some(exclude) = old.exclude {
        config.files.exclude = exclude;
    }
    Some(config.sanitised())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The file a new user is handed has to describe the app they are running.
    #[test]
    fn the_template_is_exactly_the_defaults() {
        assert_eq!(parse(TEMPLATE).unwrap(), Config::default());
    }

    #[test]
    fn an_empty_file_is_the_defaults() {
        assert_eq!(parse("").unwrap(), Config::default());
    }

    /// Notepad and PowerShell both write UTF-8 with a BOM.
    #[test]
    fn a_byte_order_mark_does_not_make_the_file_a_mistake() {
        assert_eq!(
            parse("\u{feff}[appearance]\nscale = 125")
                .unwrap()
                .appearance
                .scale,
            125
        );
    }

    #[test]
    fn a_partial_file_keeps_its_values_and_defaults_the_rest() {
        let config = parse("[editor]\ntab_width = 2\n").unwrap();
        assert_eq!(config.editor.tab_width, 2);
        assert_eq!(config.editor.font_size, 14.0);
        assert_eq!(config.appearance, Appearance::default());
        assert_eq!(config.files.exclude, [".*"]);
    }

    #[test]
    fn an_empty_exclude_list_is_respected_rather_than_defaulted() {
        assert!(parse("[files]\nexclude = []")
            .unwrap()
            .files
            .exclude
            .is_empty());
    }

    #[test]
    fn a_key_from_a_newer_build_is_ignored_rather_than_refused() {
        let config = parse("[editor]\nminimap = true\n[whatever]\nx = 1\n").unwrap();
        assert_eq!(config, Config::default());
    }

    #[test]
    fn a_mistake_says_where_it_is() {
        let err = parse("[editor]\ntab_width = \"four\"\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains("line 2"), "got {err}");
    }

    #[test]
    fn values_that_would_lose_the_window_or_the_text_are_clamped() {
        let config = parse(
            "[appearance]\nbackground_opacity = 0\nscale = 900\n\
             [editor]\nfont_size = 0\nline_height = 40\ntab_width = 0\nfont_family = \"  \"\n",
        )
        .unwrap();
        assert_eq!(config.appearance.background_opacity, 20);
        assert_eq!(config.appearance.scale, 200);
        assert_eq!(config.editor.font_size, 6.0);
        assert_eq!(config.editor.line_height, 3.0);
        assert_eq!(config.editor.tab_width, 1);
        assert_eq!(config.editor.font_family, Editor::default().font_family);
    }

    #[test]
    fn an_accent_that_is_not_a_colour_falls_back_to_the_default() {
        let accent = |text: &str| parse(text).unwrap().theme.accent;
        assert_eq!(accent("[theme]\naccent = \"#89B4FA\""), "#89b4fa");
        assert_eq!(accent("[theme]\naccent = \"blue\""), "#cba6f7");
        assert_eq!(accent("[theme]\naccent = \"#fff\""), "#cba6f7");
        assert_eq!(accent("[theme]\naccent = \"#12345g\""), "#cba6f7");
    }

    #[test]
    fn the_terminal_has_a_size_and_a_cursor_of_its_own() {
        let terminal = parse("[terminal]\nfont_size = 12\ncursor = \"bar\"")
            .unwrap()
            .terminal;
        assert_eq!(terminal.font_size, 12.0);
        assert_eq!(terminal.cursor, TerminalCursor::Bar);

        assert_eq!(
            parse("[terminal]\nfont_size = 900\nscrollback = 99999999")
                .unwrap()
                .terminal,
            Terminal {
                font_size: 72.0,
                scrollback: 100_000,
                ..Default::default()
            }
        );
        assert!(parse("[terminal]\ncursor = \"wedge\"").is_err());

        let (after, config) =
            patched(TEMPLATE, "terminal", "cursor", &json("underline".into())).unwrap();
        assert_eq!(config.terminal.cursor, TerminalCursor::Underline);
        assert!(after.contains("cursor = \"underline\""), "got {after}");
    }

    #[test]
    fn language_overrides_are_keyed_in_lower_case() {
        let config = parse("[language.Makefile]\ninsert_spaces = false\n").unwrap();
        let over = &config.language["makefile"];
        assert_eq!(over.insert_spaces, Some(false));
        assert_eq!(
            over.tab_width, None,
            "an override only says what it changes"
        );
    }

    #[test]
    fn scale_factor_is_the_percentage_as_a_multiplier() {
        let half_again = Appearance {
            scale: 150,
            ..Default::default()
        };
        assert_eq!(half_again.scale_factor(), 1.5);
    }

    fn json(value: serde_json::Value) -> serde_json::Value {
        value
    }

    /// The dialog must not cost anyone their comments or their other settings.
    #[test]
    fn setting_one_key_touches_nothing_else() {
        let before = "# mine\n[appearance]\nscale = 100 # big screen\nmaterial = \"none\"\n\n\
                      [editor]\ntab_width = 2 # two\n";
        let (after, config) = patched(before, "appearance", "scale", &json(125.into())).unwrap();

        assert!(after.starts_with("# mine\n"), "got {after}");
        assert!(after.contains("scale = 125 # big screen"), "got {after}");
        assert!(after.contains("tab_width = 2 # two"), "got {after}");
        assert_eq!(config.appearance.scale, 125);
        assert_eq!(config.editor.tab_width, 2);
    }

    #[test]
    fn every_kind_of_setting_can_be_set() {
        let set = |section, key, value: serde_json::Value| {
            patched(TEMPLATE, section, key, &value).unwrap().1
        };

        assert_eq!(
            set("appearance", "material", "acrylic".into())
                .appearance
                .material,
            Material::Acrylic
        );
        assert_eq!(
            set("editor", "font_family", "Consolas".into())
                .editor
                .font_family,
            "Consolas"
        );
        assert_eq!(
            set("editor", "line_height", 1.4.into()).editor.line_height,
            1.4
        );
        assert!(
            set("editor", "relative_line_numbers", true.into())
                .editor
                .relative_line_numbers
        );
        assert!(
            !set("editor", "insert_spaces", false.into())
                .editor
                .insert_spaces
        );
        assert_eq!(set("theme", "syntax", "other".into()).theme.syntax, "other");
        assert!(!set("vim", "enabled", false.into()).vim.enabled);
        assert_eq!(
            set("files", "exclude", serde_json::json!([".*", "target"]))
                .files
                .exclude,
            [".*", "target"]
        );
    }

    /// The template's comments describe each key; setting one must keep them.
    #[test]
    fn setting_a_key_in_the_template_keeps_its_comments() {
        let (after, _) = patched(TEMPLATE, "editor", "tab_width", &json(2.into())).unwrap();
        assert!(
            after.contains("# How wide a tab is, and how far Tab indents.\ntab_width = 2\n"),
            "got {after}"
        );
        assert_eq!(
            after.lines().count(),
            TEMPLATE.lines().count(),
            "no lines added or lost"
        );
    }

    /// The file should say what is in effect, not what was asked for.
    #[test]
    fn a_value_out_of_range_is_written_as_clamped() {
        let (after, config) = patched(TEMPLATE, "appearance", "scale", &json(900.into())).unwrap();
        assert_eq!(config.appearance.scale, 200);
        assert!(after.contains("scale = 200"), "got {after}");
    }

    #[test]
    fn a_whole_number_stays_written_as_one() {
        let (after, config) = patched(TEMPLATE, "editor", "font_size", &json(16.into())).unwrap();
        assert_eq!(config.editor.font_size, 16.0);
        assert!(after.contains("font_size = 16\n"), "got {after}");
    }

    #[test]
    fn a_value_of_the_wrong_kind_is_refused_and_says_which_setting() {
        let err = patched(TEMPLATE, "editor", "tab_width", &json("wide".into()))
            .unwrap_err()
            .to_string();
        assert!(err.contains("editor.tab_width"), "got {err}");
    }

    #[test]
    fn a_setting_that_does_not_exist_is_refused() {
        assert!(patched(TEMPLATE, "editor", "minimap", &json(true.into())).is_err());
        assert!(patched(TEMPLATE, "nowhere", "scale", &json(1.into())).is_err());
    }

    #[test]
    fn setting_a_key_repairs_a_file_with_no_such_section() {
        let (after, config) =
            patched("appearance = 3\n", "appearance", "scale", &json(125.into())).unwrap();
        assert_eq!(config.appearance.scale, 125);
        assert_eq!(parse(&after).unwrap().appearance.scale, 125);
    }

    fn mylang() -> Syntax {
        Syntax {
            extensions: vec![".MY".into(), "myl".into()],
            line_comment: "#".into(),
            block_comment: vec!["/*".into(), "*/".into()],
            strings: vec!["\"".into()],
            keywords: vec!["if".into(), "else".into(), "if".into()],
            patterns: vec![SyntaxPattern {
                pattern: r"@[a-z]+\b".into(),
                category: "builtin".into(),
            }],
            ..Default::default()
        }
    }

    #[test]
    fn a_language_is_written_into_the_file_and_read_back() {
        let (text, config) = with_syntax(TEMPLATE, "Mylang", None, Some(&mylang())).unwrap();
        assert!(text.contains("[syntax.Mylang]"), "got {text}");
        assert!(text.contains("# Thread configuration."), "comments kept");

        let stored = &config.syntax["Mylang"];
        // Tidied on the way in: no dots, lower case, nothing twice.
        assert_eq!(stored.extensions, ["my", "myl"]);
        assert_eq!(stored.keywords, ["if", "else"]);
        assert_eq!(stored.patterns[0].pattern, r"@[a-z]+\b");
        assert_eq!(parse(&text).unwrap(), config);
    }

    #[test]
    fn a_language_can_be_renamed_and_removed() {
        let (text, _) = with_syntax(TEMPLATE, "Mylang", None, Some(&mylang())).unwrap();
        let (text, config) = with_syntax(&text, "Other", Some("Mylang"), Some(&mylang())).unwrap();
        assert_eq!(config.syntax.keys().collect::<Vec<_>>(), ["Other"]);

        let (text, config) = with_syntax(&text, "Other", None, None).unwrap();
        assert!(config.syntax.is_empty());
        // Nothing left of it but the example in the template's comments.
        assert!(!text.contains("\n[syntax"), "got {text}");
        assert!(with_syntax(&text, "  ", None, Some(&mylang())).is_err());
    }

    #[test]
    fn half_a_block_comment_is_none() {
        let syntax = Syntax {
            block_comment: vec!["/*".into()],
            ..Default::default()
        };
        assert!(syntax.sanitised().block_comment.is_empty());
    }

    #[test]
    fn rendering_carries_old_settings_into_the_commented_template() {
        let mut config = Config::default();
        config.appearance.scale = 125;
        config.files.exclude = vec![".*".into(), "target".into()];

        let text = render(&config);
        assert!(text.contains("# Thread configuration."), "comments kept");
        assert_eq!(parse(&text).unwrap(), config);
    }
}
