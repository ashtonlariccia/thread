//! Settings, stored in `%APPDATA%\thread\settings.json`.
//!
//! Every field has a default, so an absent or partial `settings.json` is not
//! an error and a new field added here does not invalidate an existing file.
//!
//! The file is one flat object. [`Appearance`] is the part the Appearance
//! dialog edits; [`Settings`] is all of it.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{data_dir, strip_bom, Error, Result};

const FILE: &str = "settings.json";

/// The material drawn behind a translucent window.
///
/// `None` is a plain translucent window with nothing behind it; `Acrylic` is
/// the frosted blur Windows Terminal uses. Mica is deliberately absent: it
/// tints from the desktop wallpaper and ignores opacity, which makes it fight
/// the opacity slider rather than compose with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Material {
    #[default]
    None,
    Acrylic,
}

fn default_opacity() -> u8 {
    100
}
fn default_scale() -> u16 {
    100
}

/// What the window looks like.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Appearance {
    /// Percent. 100 is fully opaque; the window only goes translucent below it.
    #[serde(default = "default_opacity")]
    pub background_opacity: u8,

    #[serde(default)]
    pub material: Material,

    /// Percent. Zooms the interface and sizes the window to match, so 150 is
    /// the same layout half as big again rather than more room at the same size.
    #[serde(default = "default_scale")]
    pub scale: u16,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            background_opacity: default_opacity(),
            material: Material::default(),
            scale: default_scale(),
        }
    }
}

impl Appearance {
    /// Clamp anything a hand-edited file (or a future UI) could get wrong.
    ///
    /// Applied on load and on save, so a bad value can neither be stored nor
    /// reach the window. A fully transparent background makes the window
    /// unclickable-looking, and a scale near zero leaves nothing to click —
    /// both are states a user could not get out of from inside the app.
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

fn default_exclude() -> Vec<String> {
    vec![".*".into()]
}

/// Everything in the settings file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(flatten)]
    pub appearance: Appearance,

    /// File and folder names the file tree leaves out, as globs over the name
    /// (see [`crate::tree::matches`]). Dotfiles, unless this says otherwise.
    #[serde(default = "default_exclude")]
    pub exclude: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            appearance: Appearance::default(),
            exclude: default_exclude(),
        }
    }
}

/// Where the settings file lives, whether or not it exists yet.
pub fn path() -> Result<PathBuf> {
    Ok(data_dir()?.join(FILE))
}

/// The settings file's path, with every key written out in it.
///
/// For opening it in an editor. Until something is changed there is no file,
/// and one written by an older build lacks the keys added since — either way
/// the buffer would not show what there is to set. A file that does not parse
/// is left exactly as it is: it is someone's work in progress.
pub fn ensure_file() -> Result<PathBuf> {
    if let Ok(settings) = load_all() {
        write(&settings)?;
    }
    path()
}

/// The whole settings file.
pub fn load_all() -> Result<Settings> {
    let path = path()?;
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            let mut parsed: Settings = serde_json::from_str(strip_bom(&text)).map_err(|e| {
                Error::Other(anyhow::anyhow!(
                    "{} is corrupt ({e}); move it aside to start fresh",
                    path.display()
                ))
            })?;
            parsed.appearance = parsed.appearance.sanitised();
            Ok(parsed)
        }
        // No file yet is the normal first run, not a failure.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(e) => Err(Error::Io(e)),
    }
}

fn write(settings: &Settings) -> Result<()> {
    let text = serde_json::to_string_pretty(settings)
        .map_err(|e| Error::Other(anyhow::anyhow!("could not serialise settings: {e}")))?;
    std::fs::write(path()?, text)?;
    Ok(())
}

pub fn load() -> Result<Appearance> {
    Ok(load_all()?.appearance)
}

/// Store the appearance, leaving the rest of the file as it is.
pub fn save(appearance: &Appearance) -> Result<Appearance> {
    let sane = appearance.clone().sanitised();
    // A file that will not parse is replaced: the dialog has to be able to
    // dig the app out of a bad setting.
    let mut settings = load_all().unwrap_or_default();
    settings.appearance = sane.clone();
    write(&settings)?;
    Ok(sane)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_is_one_flat_object() {
        let json = serde_json::to_value(Settings::default()).unwrap();
        assert_eq!(json["scale"], 100, "appearance keys sit at the top level");
        assert_eq!(json["exclude"], serde_json::json!([".*"]));
        assert!(json.get("appearance").is_none());
    }

    #[test]
    fn a_file_from_before_exclude_existed_hides_dotfiles() {
        let parsed: Settings = serde_json::from_str(r#"{"scale": 125}"#).unwrap();
        assert_eq!(parsed.appearance.scale, 125);
        assert_eq!(parsed.exclude, [".*"]);
    }

    #[test]
    fn an_empty_exclude_list_is_respected_rather_than_defaulted() {
        let parsed: Settings = serde_json::from_str(r#"{"exclude": []}"#).unwrap();
        assert!(parsed.exclude.is_empty());
    }

    #[test]
    fn an_empty_object_yields_the_shipped_defaults() {
        let parsed: Appearance = serde_json::from_str("{}").unwrap();
        assert_eq!(parsed, Appearance::default());
    }

    /// Notepad and PowerShell both write UTF-8 with a BOM, and this file is one
    /// a user might reasonably hand-edit.
    #[test]
    fn a_byte_order_mark_does_not_make_the_file_corrupt() {
        let with_bom = "\u{feff}{\"scale\": 125}";
        assert!(
            serde_json::from_str::<Appearance>(with_bom).is_err(),
            "serde alone should choke, or this test proves nothing"
        );

        let parsed: Appearance = serde_json::from_str(strip_bom(with_bom)).unwrap();
        assert_eq!(parsed.scale, 125);
    }

    /// A file written by an older build must not lose the fields it does have.
    #[test]
    fn a_partial_file_keeps_its_values_and_defaults_the_rest() {
        let parsed: Appearance = serde_json::from_str(r#"{"scale": 125}"#).unwrap();
        assert_eq!(parsed.scale, 125);
        assert_eq!(parsed.background_opacity, 100);
        assert_eq!(parsed.material, Material::None);
    }

    /// Fully transparent is a state you cannot undo from inside the window.
    #[test]
    fn opacity_never_goes_low_enough_to_lose_the_window() {
        let ghost = Appearance {
            background_opacity: 0,
            ..Default::default()
        };
        assert_eq!(ghost.sanitised().background_opacity, 20);
    }

    #[test]
    fn scale_is_clamped_to_something_usable() {
        let tiny = Appearance {
            scale: 0,
            ..Default::default()
        };
        assert_eq!(tiny.sanitised().scale, 50);

        let huge = Appearance {
            scale: 900,
            ..Default::default()
        };
        assert_eq!(huge.sanitised().scale, 200);
    }

    #[test]
    fn scale_factor_is_the_percentage_as_a_multiplier() {
        let half_again = Appearance {
            scale: 150,
            ..Default::default()
        };
        assert_eq!(half_again.scale_factor(), 1.5);
        assert_eq!(Appearance::default().scale_factor(), 1.0);
    }
}
