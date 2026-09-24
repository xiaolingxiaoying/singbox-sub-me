//! The one deep-merge every sbctl config override uses, so the server (ADR-0021,
//! `docs/adr/0021-server-side-override-templates.md`) and the clients cannot
//! drift on what "覆写" means. The semantics, quoted from the ADR:
//!
//! - objects merge recursively; every other type replaces wholesale;
//! - arrays replace wholesale, except an array under a key literally named
//!   `rules`, which is **prepended** to the merged array so an override rule
//!   wins against the generated verdicts without restating them.
//!
//! [`MERGE_SEMANTICS`] is the shared truth table: the server, `client-core` and
//! this crate each assert against it, so changing the semantics reddens every
//! consumer's tests at once instead of silently forking one of them.

use serde_json::Value;

/// One row of the shared semantics table: merging `overlay` onto `base` must
/// produce `expected`. Documented as JSON text so a failing case prints as
/// readable JSON rather than a `Value` debug dump.
#[derive(Clone, Copy, Debug)]
pub struct MergeCase {
    pub name: &'static str,
    pub base: &'static str,
    pub overlay: &'static str,
    pub expected: &'static str,
}

/// The behaviour ADR-0021 locks, as data. Every consumer of [`deep_merge`]
/// asserts against this same list (see [`merge_semantics_failures`]).
pub const MERGE_SEMANTICS: &[MergeCase] = &[
    MergeCase {
        name: "objects-merge-recursively",
        base: r#"{"log":{"level":"info"},"keep":true}"#,
        overlay: r#"{"log":{"level":"warn","timestamp":true},"keep":false}"#,
        expected: r#"{"log":{"level":"warn","timestamp":true},"keep":false}"#,
    },
    MergeCase {
        name: "absent-keys-are-added",
        base: r#"{"dns":{"tag":"mine"}}"#,
        overlay: r#"{"dns":{"servers":["223.5.5.5"]},"experimental":{"cache_file":{"enabled":true}}}"#,
        expected: r#"{"dns":{"tag":"mine","servers":["223.5.5.5"]},"experimental":{"cache_file":{"enabled":true}}}"#,
    },
    MergeCase {
        name: "arrays-replace-by-default",
        base: r#"{"outbounds":["a"]}"#,
        overlay: r#"{"outbounds":["b","c"]}"#,
        expected: r#"{"outbounds":["b","c"]}"#,
    },
    MergeCase {
        name: "rules-arrays-are-prepended",
        base: r#"{"rules":["generated"]}"#,
        overlay: r#"{"rules":["override"]}"#,
        expected: r#"{"rules":["override","generated"]}"#,
    },
    MergeCase {
        name: "nested-dns-rules-are-prepended",
        base: r#"{"dns":{"rules":[{"generated":true}]}}"#,
        overlay: r#"{"dns":{"rules":[{"override":true}]}}"#,
        expected: r#"{"dns":{"rules":[{"override":true},{"generated":true}]}}"#,
    },
    MergeCase {
        name: "a-rules-key-that-is-not-an-array-replaces",
        base: r#"{"route":{"rules":"generated"}}"#,
        overlay: r#"{"route":{"rules":[{"action":"direct"}]}}"#,
        expected: r#"{"route":{"rules":[{"action":"direct"}]}}"#,
    },
    MergeCase {
        name: "an-object-over-a-scalar-merges-into-the-replacement",
        base: r#"{"dns":"system"}"#,
        overlay: r#"{"dns":{"servers":["1.1.1.1"]}}"#,
        expected: r#"{"dns":{"servers":["1.1.1.1"]}}"#,
    },
    MergeCase {
        name: "an-empty-overlay-changes-nothing",
        base: r#"{"log":{"level":"info"},"route":{"rules":["generated"]}}"#,
        overlay: r#"{}"#,
        expected: r#"{"log":{"level":"info"},"route":{"rules":["generated"]}}"#,
    },
];

/// Runs [`MERGE_SEMANTICS`] through [`deep_merge`] and returns the names of the
/// rows that do not hold, so a consumer's test can assert on the shared table
/// without restating it. An empty vector means the semantics are intact.
pub fn merge_semantics_failures() -> Vec<&'static str> {
    MERGE_SEMANTICS
        .iter()
        .filter(|case| {
            let mut base: Value =
                serde_json::from_str(case.base).expect("the shared table holds valid JSON base");
            let overlay: Value = serde_json::from_str(case.overlay)
                .expect("the shared table holds valid JSON overlay");
            let expected: Value =
                serde_json::from_str(case.expected).expect("the shared table holds valid JSON");
            deep_merge(&mut base, &overlay);
            base != expected
        })
        .map(|case| case.name)
        .collect()
}

/// Deep-merges `overlay` into `base` in place. Objects merge recursively and
/// other types replace — except an array under a key literally named `rules`,
/// which the overlay prepends to so an override rule wins over the generated
/// verdicts without restating the whole list.
pub fn deep_merge(base: &mut Value, overlay: &Value) {
    match (base, overlay) {
        (Value::Object(base_map), Value::Object(overlay_map)) => {
            for (key, value) in overlay_map {
                match base_map.get_mut(key) {
                    Some(existing) => {
                        if key == "rules" && existing.is_array() && value.is_array() {
                            let Value::Array(base_rules) = existing else {
                                unreachable!("checked is_array above")
                            };
                            let Value::Array(overlay_rules) = value else {
                                unreachable!("checked is_array above")
                            };
                            let mut merged = overlay_rules.clone();
                            merged.extend(base_rules.iter().cloned());
                            *existing = Value::Array(merged);
                            continue;
                        }
                        deep_merge(existing, value);
                    }
                    None => {
                        base_map.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        (base, overlay) => *base = overlay.clone(),
    }
}

/// The YAML twin of [`deep_merge`], operating on `serde_yaml::Value`. Same
/// semantics, same `rules` prepend, and the same table behind it: see
/// [`merge_semantics_yaml_failures`].
#[cfg(feature = "yaml")]
pub fn deep_merge_yaml(base: &mut serde_yaml::Value, overlay: &serde_yaml::Value) {
    use serde_yaml::Value as Yaml;
    match (base, overlay) {
        (Yaml::Mapping(base_map), Yaml::Mapping(overlay_map)) => {
            for (key, value) in overlay_map {
                match base_map.get_mut(key) {
                    Some(existing) => {
                        if key.as_str() == Some("rules")
                            && existing.is_sequence()
                            && value.is_sequence()
                        {
                            let Yaml::Sequence(base_rules) = existing else {
                                unreachable!("checked is_sequence above")
                            };
                            let Yaml::Sequence(overlay_rules) = value else {
                                unreachable!("checked is_sequence above")
                            };
                            let mut merged = overlay_rules.clone();
                            merged.extend(base_rules.iter().cloned());
                            *existing = Yaml::Sequence(merged);
                            continue;
                        }
                        deep_merge_yaml(existing, value);
                    }
                    None => {
                        base_map.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        (base, overlay) => *base = overlay.clone(),
    }
}

/// The same shared table, run through [`deep_merge_yaml`] by round-tripping each
/// row through YAML. A row whose JSON is not expressible as a YAML mapping at
/// the top level is skipped rather than pretended to be covered.
#[cfg(feature = "yaml")]
pub fn merge_semantics_yaml_failures() -> Vec<&'static str> {
    MERGE_SEMANTICS
        .iter()
        .filter(|case| {
            let Ok(mut base) = serde_yaml::from_str::<serde_yaml::Value>(case.base) else {
                return false;
            };
            let Ok(overlay) = serde_yaml::from_str::<serde_yaml::Value>(case.overlay) else {
                return false;
            };
            let Ok(expected) = serde_yaml::from_str::<serde_yaml::Value>(case.expected) else {
                return false;
            };
            deep_merge_yaml(&mut base, &overlay);
            base != expected
        })
        .map(|case| case.name)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_shared_table_holds_for_json() {
        assert_eq!(merge_semantics_failures(), Vec::<&str>::new());
    }

    #[test]
    fn merging_is_in_place_and_leaves_the_overlay_usable() {
        let overlay = json!({"route": {"rules": [{"action": "direct"}]}});
        let mut base = json!({"route": {"rules": [{"action": "proxy"}]}});
        deep_merge(&mut base, &overlay);
        deep_merge(&mut base, &overlay);
        assert_eq!(
            base,
            json!({"route": {"rules": [{"action": "direct"}, {"action": "direct"}, {"action": "proxy"}]}}),
            "the overlay must not be consumed by the merge"
        );
    }

    #[cfg(feature = "yaml")]
    #[test]
    fn the_shared_table_holds_for_yaml() {
        assert_eq!(merge_semantics_yaml_failures(), Vec::<&str>::new());
    }

    #[cfg(feature = "yaml")]
    #[test]
    fn yaml_rules_are_prepended() {
        let mut base = serde_yaml::from_str("rules:\n  - generated\n").expect("yaml");
        let overlay = serde_yaml::from_str("rules:\n  - override\n").expect("yaml");
        deep_merge_yaml(&mut base, &overlay);
        let text = serde_yaml::to_string(&base).expect("yaml round-trip");
        assert!(text.contains("- override\n- generated"), "{text}");
    }
}
