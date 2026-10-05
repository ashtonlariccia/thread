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
pub const TEMPLATE: &str = r#"# Thread configuration.
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

[files]
# Names the file tree leaves out. `*` is any run of characters and `?` is any
# one, matched against the name alone, so ".*" is every dotfile at any depth.
exclude = [".*"]

[theme]
# Colours for syntax highlighting. Available: "catppuccin".
syntax = "catppuccin"

# Per-language overrides for `tab_width` and `insert_spaces`. The name is the
# one shown in the bottom bar, in lower case. Detected indentation still wins
# while `detect_indentation` is on.
#
# [language.makefile]
# insert_spaces = false
#
# [language.yaml]
# tab_width = 2
"#;

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
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            syntax: "catppuccin".into(),
        }
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
    /// By language name, lower-case.
    pub language: BTreeMap<String, LanguageOverride>,
}

impl Config {
    fn sanitised(mut self) -> Self {
        self.appearance = self.appearance.sanitised();
        self.editor = self.editor.sanitised();
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
