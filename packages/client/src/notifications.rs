//! Notification toggle ↔ Matrix push-rule mapping (checkpoint 10).
//!
//! The settings UI shows friendly toggles; Matrix push rules are the storage.
//! Some toggles cover more than one rule ("direct messages" spans the plain
//! and the encrypted one-to-one rule), so the mapping is a table, not a 1:1
//! enum — and it lives here, plain-data and matrix-free, so the mapping itself
//! is unit-testable without linking matrix-sdk (the `matrix` impl maps
//! [`RuleKind`] onto ruma's `RuleKind` in `runtime.rs`).
//!
//! Toggle semantics: a toggle reads **enabled** when all its rules are
//! enabled, and setting it writes all of them. The one deliberate exception
//! is the master rule: `.m.rule.master` is inverted in spirit — it *disables*
//! all notifications — but the UI model stays uniform ("enabled" = rule
//! enabled) and the label carries the meaning ("Mute all notifications" with
//! the toggle meaning "this rule fires"); see the notes on `master`.

/// Matrix push-rule kinds the table needs, as plain data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuleKind {
    Override,
    Underride,
}

/// One Matrix push rule referenced by a toggle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleRef {
    pub kind: RuleKind,
    /// Canonical rule id, e.g. `.m.rule.master`.
    pub rule_id: &'static str,
}

/// One settings toggle and the rules behind it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToggleDef {
    /// Stable toggle key crossing the trait seam (`NotifToggle::id`).
    pub id: &'static str,
    pub label: &'static str,
    pub rules: &'static [RuleRef],
    /// Sensible default for a toggle whose rules the server hasn't reported.
    pub default: bool,
}

const MASTER: RuleRef = RuleRef {
    kind: RuleKind::Override,
    rule_id: ".m.rule.master",
};
const CONTAINS_DISPLAY_NAME: RuleRef = RuleRef {
    kind: RuleKind::Override,
    rule_id: ".m.rule.contains_display_name",
};
const ROOM_ONE_TO_ONE: RuleRef = RuleRef {
    kind: RuleKind::Underride,
    rule_id: ".m.rule.room_one_to_one",
};
const ENCRYPTED_ROOM_ONE_TO_ONE: RuleRef = RuleRef {
    kind: RuleKind::Underride,
    rule_id: ".m.rule.encrypted_room_one_to_one",
};
const ENCRYPTED: RuleRef = RuleRef {
    kind: RuleKind::Underride,
    rule_id: ".m.rule.encrypted",
};

/// The whole surface the Notifications tab shows (checkpoint 10 scope:
/// master mute, mentions, DMs, encrypted rooms).
pub const RULE_TABLE: &[ToggleDef] = &[
    ToggleDef {
        id: "master",
        label: "Mute all notifications",
        rules: &[MASTER],
        // Spec default: master rule present but disabled (not muted).
        default: false,
    },
    ToggleDef {
        id: "mentions",
        label: "Mentions of my display name",
        rules: &[CONTAINS_DISPLAY_NAME],
        default: true,
    },
    ToggleDef {
        id: "dms",
        label: "Direct messages",
        rules: &[ROOM_ONE_TO_ONE, ENCRYPTED_ROOM_ONE_TO_ONE],
        default: true,
    },
    ToggleDef {
        id: "encrypted_rooms",
        label: "Encrypted rooms",
        rules: &[ENCRYPTED],
        default: true,
    },
];

/// Look a toggle up by its seam id (returns `None` for unknown ids).
pub fn toggle_def(id: &str) -> Option<&'static ToggleDef> {
    RULE_TABLE.iter().find(|t| t.id == id)
}

/// Default-enabled toggles as `NotifToggle`s — the mock's seed state and the
/// fallback when the server hasn't reported a ruleset yet.
pub fn default_toggles() -> Vec<crate::model::NotifToggle> {
    RULE_TABLE
        .iter()
        .map(|t| crate::model::NotifToggle {
            id: t.id.to_string(),
            label: t.label.to_string(),
            enabled: t.default,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn toggle_ids_are_unique_and_stable() {
        let ids: HashSet<_> = RULE_TABLE.iter().map(|t| t.id).collect();
        assert_eq!(ids.len(), RULE_TABLE.len(), "duplicate toggle ids");
        for id in ["master", "mentions", "dms", "encrypted_rooms"] {
            assert!(toggle_def(id).is_some(), "missing toggle {id}");
        }
    }

    #[test]
    fn every_toggle_maps_to_at_least_one_rule() {
        for t in RULE_TABLE {
            assert!(!t.rules.is_empty(), "{} has no rules", t.id);
            for r in t.rules {
                assert!(
                    r.rule_id.starts_with(".m.rule."),
                    "non-canonical {}",
                    r.rule_id
                );
            }
        }
    }

    #[test]
    fn dms_spans_plain_and_encrypted_one_to_one() {
        let dms = toggle_def("dms").expect("dms");
        let ids: Vec<_> = dms.rules.iter().map(|r| r.rule_id).collect();
        assert!(ids.contains(&".m.rule.room_one_to_one"));
        assert!(ids.contains(&".m.rule.encrypted_room_one_to_one"));
        assert_eq!(dms.rules.len(), 2);
    }

    #[test]
    fn no_rule_is_claimed_by_two_toggles() {
        let mut seen = HashSet::new();
        for t in RULE_TABLE {
            for r in t.rules {
                assert!(
                    seen.insert((r.kind, r.rule_id)),
                    "{} double-booked",
                    r.rule_id
                );
            }
        }
    }

    #[test]
    fn default_toggles_cover_the_whole_table() {
        let defaults = default_toggles();
        assert_eq!(defaults.len(), RULE_TABLE.len());
        let master = defaults.iter().find(|t| t.id == "master").unwrap();
        assert!(!master.enabled, "fresh accounts are not muted by default");
        assert!(defaults.iter().find(|t| t.id == "dms").unwrap().enabled);
    }

    #[test]
    fn unknown_toggle_lookup_is_none() {
        assert!(toggle_def("nope").is_none());
    }
}
