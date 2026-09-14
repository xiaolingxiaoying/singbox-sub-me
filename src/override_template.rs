//! Server-side override templates merged into the generated client artifacts.
//!
//! Overrides live under `etc/sbctl/overrides/` inside the deployment root:
//! `sing-box-override.json` (JSON, deep-merged into every full sing-box client
//! profile) and `clash-override.yaml` (YAML, deep-merged into the current and
//! legacy clash artifacts). Merge semantics, documented in ADR-0021:
//!
//! - objects merge recursively; every other type replaces wholesale;
//! - arrays replace wholesale, except an array under a key literally named
//!   `rules`, which is **prepended** to the generated array so an override
//!   rule can win against the generated verdicts without restating them;
//! - the historical bare-`outbounds` sing-box artifact and the URI artifacts
//!   are never overridden, so their byte compatibility is preserved.

use std::fs;
use std::path::{Path, PathBuf};

pub const SING_BOX_OVERRIDE_RELATIVE_PATH: &str = "etc/sbctl/overrides/sing-box-override.json";
pub const CLASH_OVERRIDE_RELATIVE_PATH: &str = "etc/sbctl/overrides/clash-override.yaml";

#[derive(Debug, thiserror::Error)]
pub enum OverrideError {
    #[error("{path} is not valid JSON: {message}")]
    Json { path: PathBuf, message: String },
    #[error("{path} is not valid YAML: {message}")]
    Yaml { path: PathBuf, message: String },
    #[error("{path} must contain a JSON object or YAML mapping at the top level")]
    NotMapping { path: PathBuf },
    #[error("override read failed: {0}")]
    Io(#[from] std::io::Error),
}

/// The parsed override documents for one deployment; empty when none exist.
#[derive(Debug, Default)]
pub struct Overrides {
    pub sing_box: Option<serde_json::Value>,
    pub clash: Option<serde_yaml::Value>,
}

impl Overrides {
    /// Loads both override documents from `<root>/etc/sbctl/overrides/`.
    /// A malformed file aborts generation so the transactional writer keeps
    /// the previous known-good artifacts.
    pub fn load(root: &Path) -> Result<Self, OverrideError> {
        let sing_box_path = root.join(SING_BOX_OVERRIDE_RELATIVE_PATH);
        let clash_path = root.join(CLASH_OVERRIDE_RELATIVE_PATH);
        let sing_box = load_json_if_present(&sing_box_path)?;
        let clash = load_yaml_if_present(&clash_path)?;
        Ok(Self { sing_box, clash })
    }
}

fn load_json_if_present(path: &Path) -> Result<Option<serde_json::Value>, OverrideError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(OverrideError::Io(error)),
    };
    if text.trim().is_empty() {
        return Ok(None);
    }
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|error| OverrideError::Json {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
    match value {
        serde_json::Value::Object(_) => Ok(Some(value)),
        _ => Err(OverrideError::NotMapping {
            path: path.to_path_buf(),
        }),
    }
}

fn load_yaml_if_present(path: &Path) -> Result<Option<serde_yaml::Value>, OverrideError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(OverrideError::Io(error)),
    };
    if text.trim().is_empty() {
        return Ok(None);
    }
    let value: serde_yaml::Value =
        serde_yaml::from_str(&text).map_err(|error| OverrideError::Yaml {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
    match value {
        serde_yaml::Value::Mapping(_) => Ok(Some(value)),
        _ => Err(OverrideError::NotMapping {
            path: path.to_path_buf(),
        }),
    }
}

/// Deep-merges `overlay` into `base` in place. Objects merge recursively and
/// other types replace — except an array under a key literally named `rules`,
/// which the overlay prepends to so an override rule wins over the generated
/// verdicts without restating the whole list.
pub fn deep_merge(base: &mut serde_json::Value, overlay: &serde_json::Value) {
    match (base, overlay) {
        (serde_json::Value::Object(base_map), serde_json::Value::Object(overlay_map)) => {
            for (key, value) in overlay_map {
                match base_map.get_mut(key) {
                    Some(existing) => {
                        if key == "rules" && existing.is_array() && value.is_array() {
                            let serde_json::Value::Array(base_rules) = existing else {
                                unreachable!("checked is_array above")
                            };
                            let serde_json::Value::Array(overlay_rules) = value else {
                                unreachable!("checked is_array above")
                            };
                            let mut merged = overlay_rules.clone();
                            merged.extend(base_rules.iter().cloned());
                            *existing = serde_json::Value::Array(merged);
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

/// The YAML twin of [`deep_merge`], operating on `serde_yaml::Value`.
pub fn deep_merge_yaml(base: &mut serde_yaml::Value, overlay: &serde_yaml::Value) {
    match (base, overlay) {
        (serde_yaml::Value::Mapping(base_map), serde_yaml::Value::Mapping(overlay_map)) => {
            for (key, value) in overlay_map {
                match base_map.get_mut(key) {
                    Some(existing) => {
                        if key == "rules" && existing.is_sequence() && value.is_sequence() {
                            let serde_yaml::Value::Sequence(base_rules) = existing else {
                                unreachable!("checked is_sequence above")
                            };
                            let serde_yaml::Value::Sequence(overlay_rules) = value else {
                                unreachable!("checked is_sequence above")
                            };
                            let mut merged = overlay_rules.clone();
                            merged.extend(base_rules.iter().cloned());
                            *existing = serde_yaml::Value::Sequence(merged);
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn objects_merge_recursively_and_scalars_replace() {
        let mut base = json!({"log": {"level": "info"}, "keep": true});
        let overlay = json!({"log": {"level": "warn", "timestamp": true}, "keep": false});
        deep_merge(&mut base, &overlay);
        assert_eq!(
            base,
            json!({"log": {"level": "warn", "timestamp": true}, "keep": false})
        );
    }

    #[test]
    fn arrays_replace_by_default() {
        let mut base = json!({"outbounds": ["a"]});
        deep_merge(&mut base, &json!({"outbounds": ["b", "c"]}));
        assert_eq!(base["outbounds"], json!(["b", "c"]));
    }

    #[test]
    fn rules_arrays_are_prepended_including_nested_dns_rules() {
        let mut base = json!({"dns": {"rules": [{"generated": true}]}});
        deep_merge(&mut base, &json!({"dns": {"rules": [{"override": true}]}}));
        assert_eq!(
            base["dns"]["rules"],
            json!([{"override": true}, {"generated": true}])
        );
        let mut base = json!({"rules": ["generated"]});
        deep_merge(&mut base, &json!({"rules": ["override"]}));
        assert_eq!(base["rules"], json!(["override", "generated"]));
    }

    #[test]
    fn missing_files_are_empty_and_malformed_documents_are_rejected() {
        use std::fs;

        let directory = tempfile::tempdir().expect("temporary root is created");
        let root = directory.path();
        let empty = Overrides::load(root).expect("missing override files load as empty");
        assert!(empty.sing_box.is_none() && empty.clash.is_none());

        fs::create_dir_all(root.join("etc/sbctl/overrides"))
            .expect("override directory is created");
        let sing_box = root.join(SING_BOX_OVERRIDE_RELATIVE_PATH);
        fs::write(&sing_box, "{ not json").expect("malformed JSON is written");
        assert!(matches!(
            Overrides::load(root),
            Err(OverrideError::Json { .. })
        ));

        fs::write(&sing_box, "[]").expect("non-mapping JSON is written");
        assert!(matches!(
            Overrides::load(root),
            Err(OverrideError::NotMapping { .. })
        ));

        fs::remove_file(&sing_box).expect("JSON override is removed");
        fs::write(
            root.join(CLASH_OVERRIDE_RELATIVE_PATH),
            "- just\n- a\n- list\n",
        )
        .expect("non-mapping YAML is written");
        assert!(matches!(
            Overrides::load(root),
            Err(OverrideError::NotMapping { .. })
        ));
    }

    #[test]
    fn yaml_rules_are_prepended() {
        let mut base = serde_yaml::from_str("rules:\n  - generated\n").expect("yaml");
        let overlay = serde_yaml::from_str("rules:\n  - override\n").expect("yaml");
        deep_merge_yaml(&mut base, &overlay);
        let text = serde_yaml::to_string(&base).expect("yaml round-trip");
        assert!(text.contains("- override\n- generated"), "{text}");
    }
}
