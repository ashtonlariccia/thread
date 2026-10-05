//! Pins — the strip along the bottom of the window.
//!
//! Stored in `%APPDATA%\thread\pins.json`. Two rules shape this file:
//!
//! * **A pin stores what to open, never how it looks.** `kind` says what sort
//!   of thing it is and `target` identifies it; [`Pin::label`] is the name at
//!   the time of pinning. Nothing produces pins yet, so `kind` is an open
//!   string rather than an enum — each feature that learns to pin claims one.
//! * **One row per target.** Pinning something already pinned refreshes its
//!   stored label instead of adding a second, identical button.

use serde::{Deserialize, Serialize};

use crate::{data_dir, strip_bom, Error, Result};

const FILE: &str = "pins.json";

/// One button on the strip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pin {
    pub kind: String,
    /// Whatever identifies the thing within its [`Pin::kind`].
    pub target: String,
    #[serde(default)]
    pub label: String,
}

impl Pin {
    /// Pins are the same button if they open the same thing. The label is
    /// deliberately not part of this — a renamed target is not a new pin.
    fn points_at(&self, kind: &str, target: &str) -> bool {
        self.kind == kind && self.target == target
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Store {
    #[serde(default)]
    pins: Vec<Pin>,
}

impl Store {
    pub fn load() -> Result<Self> {
        let path = data_dir()?.join(FILE);
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(strip_bom(&text)).map_err(|e| {
                Error::Other(anyhow::anyhow!(
                    "{} is corrupt ({e}); move it aside to start fresh",
                    path.display()
                ))
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error::Io(e)),
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = data_dir()?.join(FILE);
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| Error::Other(anyhow::anyhow!("could not serialise pins: {e}")))?;
        std::fs::write(&path, text)?;
        Ok(())
    }

    /// Every pin, in the order they were added — which is the order they appear
    /// in the strip, so it must stay stable.
    pub fn list(&self) -> Vec<Pin> {
        self.pins.clone()
    }

    /// Add a pin, or refresh the stored label of the one already holding that
    /// target. Returns whether the strip actually gained a button.
    pub fn add(&mut self, pin: Pin) -> bool {
        if let Some(existing) = self
            .pins
            .iter_mut()
            .find(|p| p.points_at(&pin.kind, &pin.target))
        {
            existing.label = pin.label;
            return false;
        }

        self.pins.push(pin);
        true
    }

    /// Move a pin to `index`, counted in the list *as it will be afterwards*.
    ///
    /// Applied to whatever is on disk right now rather than by overwriting the
    /// order wholesale, so a second window that pinned something in the
    /// meantime does not lose it to a stale list. Returns false if that pin
    /// isn't there.
    pub fn move_to(&mut self, kind: &str, target: &str, index: usize) -> bool {
        let Some(from) = self.pins.iter().position(|p| p.points_at(kind, target)) else {
            return false;
        };

        let pin = self.pins.remove(from);
        // Clamped against the shortened list: dropping past the right-hand end
        // is a normal gesture, not an error.
        self.pins.insert(index.min(self.pins.len()), pin);
        true
    }

    pub fn remove(&mut self, kind: &str, target: &str) -> bool {
        let before = self.pins.len();
        self.pins.retain(|p| !p.points_at(kind, target));
        self.pins.len() != before
    }

    pub fn contains(&self, kind: &str, target: &str) -> bool {
        self.pins.iter().any(|p| p.points_at(kind, target))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pin(kind: &str, target: &str, label: &str) -> Pin {
        Pin {
            kind: kind.into(),
            target: target.into(),
            label: label.into(),
        }
    }

    #[test]
    fn pinning_the_same_target_twice_does_not_duplicate() {
        let mut store = Store::default();
        assert!(store.add(pin("file", "a.rs", "a.rs")));
        assert!(!store.add(pin("file", "a.rs", "a.rs")));
        assert_eq!(store.list().len(), 1);
    }

    #[test]
    fn re_pinning_refreshes_the_stored_label_in_place() {
        let mut store = Store::default();
        store.add(pin("file", "a.rs", "a.rs"));
        store.add(pin("file", "a.rs", "main entry"));

        let pins = store.list();
        assert_eq!(pins.len(), 1, "a rename is not a new pin");
        assert_eq!(pins[0].label, "main entry");
    }

    #[test]
    fn kind_is_part_of_the_identity() {
        // Two kinds could share a target string; they are still two buttons.
        let mut store = Store::default();
        assert!(store.add(pin("file", "x", "a file")));
        assert!(store.add(pin("folder", "x", "a folder")));
        assert_eq!(store.list().len(), 2);
        assert!(store.contains("folder", "x"));
    }

    /// The strip's order after `targets` have been pinned in that sequence.
    fn strip(targets: &[&str]) -> Store {
        let mut store = Store::default();
        for t in targets {
            store.add(pin("file", t, t));
        }
        store
    }

    fn order(store: &Store) -> Vec<String> {
        store.list().into_iter().map(|p| p.target).collect()
    }

    #[test]
    fn order_is_the_order_they_were_pinned() {
        assert_eq!(order(&strip(&["a", "b", "c"])), ["a", "b", "c"]);
    }

    #[test]
    fn a_pin_can_be_dragged_left_and_right() {
        let mut store = strip(&["a", "b", "c", "d"]);

        assert!(store.move_to("file", "d", 0));
        assert_eq!(order(&store), ["d", "a", "b", "c"]);

        assert!(store.move_to("file", "d", 2));
        assert_eq!(order(&store), ["a", "b", "d", "c"]);
    }

    #[test]
    fn dropping_past_the_end_lands_at_the_end() {
        let mut store = strip(&["a", "b", "c"]);

        // The gesture is "drop it to the right of everything", which should not
        // need the caller to know how long the list is.
        assert!(store.move_to("file", "a", 99));
        assert_eq!(order(&store), ["b", "c", "a"]);
    }

    #[test]
    fn moving_never_duplicates_or_drops_a_pin() {
        // The failure that would matter: an index off by one that leaves the
        // dragged pin in twice, or not at all.
        let mut store = strip(&["a", "b", "c", "d", "e"]);
        for index in 0..5 {
            assert!(store.move_to("file", "c", index));
            let mut seen = order(&store);
            seen.sort();
            assert_eq!(
                seen,
                ["a", "b", "c", "d", "e"],
                "lost or duplicated at {index}"
            );
        }
    }

    #[test]
    fn moving_something_that_is_not_pinned_reports_nothing_done() {
        let mut store = strip(&["a"]);
        assert!(!store.move_to("file", "nope", 0));
        assert!(
            !store.move_to("folder", "a", 0),
            "kind is part of the match"
        );
        assert_eq!(order(&store), ["a"]);
    }

    #[test]
    fn removing_works_and_is_idempotent() {
        let mut store = strip(&["a"]);

        assert!(store.remove("file", "a"));
        assert!(!store.remove("file", "a"));
        assert!(store.list().is_empty());
    }

    #[test]
    fn an_empty_file_is_an_empty_strip() {
        let store: Store = serde_json::from_str("{}").unwrap();
        assert!(store.list().is_empty());
    }

    #[test]
    fn pins_round_trip_through_json() {
        let store = strip(&["a"]);

        let json = serde_json::to_string(&store).unwrap();
        let back: Store = serde_json::from_str(&json).unwrap();
        assert_eq!(back.list(), store.list());
    }
}
