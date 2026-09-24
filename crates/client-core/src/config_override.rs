//! Per-profile configuration overrides: 覆写配置文件内容.
//!
//! One file per profile, `<data dir>/overrides/<profile-sha256>.json`, kept
//! next to but outside the subscription cache so an update cannot roll over the
//! user's own work. The document has two accepted shapes:
//!
//! ```text
//! { "fragments": [ { "id": "private-direct", "label": "内网直连",
//!                    "enabled": true, "overlay": { "route": { "rules": [...] } } } ] }
//! ```
//!
//! …or, for a hand-written file, a bare sing-box-shaped object
//! (`{ "dns": … }`), which reads as one always-enabled implicit fragment.
//!
//! What the merge does is the server's contract and nothing else (ADR-0021):
//! [`json_merge::deep_merge`] — objects merge recursively, arrays replace, an
//! array under a key named `rules` is prepended so the override rule wins
//! without restating the generated verdicts. Enabled fragments are merged
//! last-to-first, so the fragment listed first holds the top of the rule order
//! and wins any key two fragments both touch.
//!
//! The merge can never take the control channel away: [`RESERVED_POINTERS`] are
//! the fields the client itself depends on, and [`ProfileOverride::apply`]
//! reports every fragment that tried to change one, after which
//! [`crate::core::runtime_config`] writes them back. Reporting rather than
//! silence is the point — an override that quietly stopped working is worse
//! than one that says it was refused.

use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use json_merge::deep_merge;
use serde_json::Value;

/// Where the per-profile override files live inside the data directory.
pub const OVERRIDE_DIRECTORY: &str = "overrides";
/// The top-level key holding the fragment list.
pub const FRAGMENTS_KEY: &str = "fragments";
/// The id of the implicit fragment a bare-object file parses to. It is not a
/// real id: [`ProfileOverride::toggle`] refuses to rewrite a hand-written file.
pub const IMPLICIT_FRAGMENT_ID: &str = "override";

/// The fields the client re-asserts after any merge, as JSON pointer segments.
///
/// The `clash_api` pair is how the engine talks to the core it started: an
/// override that moved the endpoint or the secret would leave the UI polling a
/// port nobody owns. `route.auto_detect_interface` decides whether traffic can
/// reach the core at all — in TUN mode a profile that turns it off routes the
/// tunnel into itself. `inbounds` is the client's own list: the traffic-mode
/// switch (system proxy vs TUN) *replaces* it wholesale in
/// [`crate::core::adapt_inbounds`], so an override's inbound would simply
/// vanish at the next step and the user would chase a bug that is by design.
pub const RESERVED_POINTERS: &[&[&str]] = &[
    &["experimental", "clash_api", "external_controller"],
    &["experimental", "clash_api", "secret"],
    &["route", "auto_detect_interface"],
    &["inbounds"],
];

/// One named, individually switchable piece of an override.
#[derive(Clone, Debug, PartialEq)]
pub struct RuleFragment {
    /// Stable handle the UI toggles by; unique inside one document.
    pub id: String,
    /// The label shown to the user; defaults to the id.
    pub label: String,
    pub enabled: bool,
    /// The sing-box-shaped document merged when this fragment is enabled.
    pub overlay: Value,
}

/// One profile's parsed override file.
#[derive(Clone, Debug, PartialEq)]
pub struct ProfileOverride {
    pub path: PathBuf,
    /// Size of the file on disk, for the status line.
    pub bytes: u64,
    /// True when the file was a bare object: one fragment, nothing to toggle.
    pub implicit: bool,
    pub fragments: Vec<RuleFragment>,
}

/// Renders a JSON pointer path for messages and UI display.
pub fn pointer(parts: &[&str]) -> String {
    if parts.is_empty() {
        return "/".to_owned();
    }
    format!("/{}", parts.join("/"))
}

/// Why a pointer is reserved, in the words the UI prints.
pub fn reserved_reason(path: &str) -> &'static str {
    match path {
        "/experimental/clash_api/external_controller" => "控制通道地址由客户端随机分配",
        "/experimental/clash_api/secret" => "控制通道密钥由客户端随机生成",
        "/route/auto_detect_interface" => "本地运行必须自动识别出口网卡",
        "/inbounds" => "入站由流量模式整体决定（系统代理 / TUN）",
        _ => "客户端保留字段",
    }
}

impl ProfileOverride {
    /// Reads one profile's override file. A missing or blank file is `None`,
    /// which means "no override" rather than an error; a malformed file is an
    /// error naming the field path, and the caller must not apply anything.
    pub fn load(path: &Path) -> Result<Option<Self>> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => bail!("读取 {} 失败: {error}", path.display()),
        };
        if text.trim().is_empty() {
            return Ok(None);
        }
        let bytes = text.len() as u64;
        let mut parsed = Self::parse(&text, path)?;
        parsed.bytes = bytes;
        Ok(Some(parsed))
    }

    /// Parses override text, reporting the offending field path.
    pub fn parse(text: &str, path: &Path) -> Result<Self> {
        let value: Value = serde_json::from_str(text).map_err(|error| {
            anyhow::anyhow!(
                "{} 不是有效的 JSON（根对象 `/`，第 {} 行第 {} 列）: {error}",
                path.display(),
                error.line(),
                error.column()
            )
        })?;
        let object = value
            .as_object()
            .with_context(|| format!("{} 的顶层（`/`）必须是 JSON 对象", path.display()))?;
        let mut parsed = Self {
            path: path.to_path_buf(),
            bytes: text.len() as u64,
            implicit: false,
            fragments: Vec::new(),
        };
        let Some(fragments) = object.get(FRAGMENTS_KEY) else {
            // The hand-written shape: the whole document *is* the overlay.
            parsed.implicit = true;
            parsed.fragments.push(RuleFragment {
                id: IMPLICIT_FRAGMENT_ID.to_owned(),
                label: "整份覆写（手写文件）".to_owned(),
                enabled: true,
                overlay: value,
            });
            return Ok(parsed);
        };
        let list = fragments
            .as_array()
            .with_context(|| format!("{} 的 `/fragments` 必须是数组", path.display()))?;
        for (index, entry) in list.iter().enumerate() {
            parsed.fragments.push(parse_fragment(entry, path, index)?);
        }
        let mut seen: Vec<&str> = Vec::new();
        for fragment in &parsed.fragments {
            if seen.contains(&fragment.id.as_str()) {
                bail!(
                    "{} 的片段 id `{}` 重复；片段必须可寻址才能开关",
                    path.display(),
                    fragment.id
                );
            }
            seen.push(&fragment.id);
        }
        Ok(parsed)
    }

    /// The fragments that will actually be merged, in the order the UI lists
    /// them (highest precedence first).
    pub fn enabled(&self) -> Vec<&RuleFragment> {
        self.fragments.iter().filter(|f| f.enabled).collect()
    }

    /// Merges every enabled fragment into `base` in place and returns the
    /// reserved pointers the file tried to change. The caller still owns those
    /// fields and is expected to write them back after this returns.
    pub fn apply(&self, base: &mut Value) -> Vec<String> {
        let mut conflicts: Vec<String> = Vec::new();
        // Last-to-first, so the first fragment ends up on top of every `rules`
        // array and keeps any scalar key the fragments share.
        for fragment in self.fragments.iter().filter(|f| f.enabled).rev() {
            deep_merge(base, &fragment.overlay);
            conflicts.extend(fragment.reserved_conflicts());
        }
        conflicts.sort();
        conflicts.dedup();
        conflicts
    }

    /// Every reserved pointer any enabled fragment touches.
    pub fn reserved_conflicts(&self) -> Vec<String> {
        let mut conflicts: Vec<String> = self
            .enabled()
            .into_iter()
            .flat_map(|fragment| fragment.reserved_conflicts())
            .collect();
        conflicts.sort();
        conflicts.dedup();
        conflicts
    }

    /// The field paths the enabled fragments change, for "覆写改动了什么" and
    /// for prefixing a failed `sing-box check` with where to look.
    pub fn changed_pointers(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for fragment in self.enabled() {
            out.extend(fragment.changed_pointers());
        }
        out.sort();
        out.dedup();
        out
    }

    /// Flips one fragment's enabled flag. Errors instead of rewriting a
    /// hand-written bare object, because there is no fragment list in it and
    /// inventing one would replace the user's document with ours.
    pub fn toggle(&mut self, id: &str, enabled: Option<bool>) -> Result<bool> {
        if self.implicit {
            bail!(
                "{} 是手写整份覆写（无片段列表），不能单独开关；改成 `fragments` 数组后可以开关",
                self.path.display()
            );
        }
        let known: Vec<&str> = self
            .fragments
            .iter()
            .map(|fragment| fragment.id.as_str())
            .collect();
        let Some(position) = known.iter().position(|candidate| *candidate == id) else {
            bail!(
                "{} 没有 id 为 `{id}` 的片段（现有片段：{}）",
                self.path.display(),
                if known.is_empty() {
                    "无".to_owned()
                } else {
                    known.join("、")
                }
            );
        };
        let fragment = &mut self.fragments[position];
        fragment.enabled = enabled.unwrap_or(!fragment.enabled);
        Ok(fragment.enabled)
    }

    /// The snapshot-facing view of this file: which profile owns it, how big it
    /// is, and per fragment what it changes and which reserved fields it tries
    /// to touch. Nothing here depends on the core running, so the panel can be
    /// honest before the first start.
    pub fn summary(&self, profile: &str) -> crate::state::OverrideSummary {
        crate::state::OverrideSummary {
            profile: profile.to_owned(),
            file_name: self
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            bytes: self.bytes,
            implicit: self.implicit,
            fragments: self
                .fragments
                .iter()
                .map(|fragment| crate::state::OverrideFragmentSummary {
                    id: fragment.id.clone(),
                    label: fragment.label.clone(),
                    enabled: fragment.enabled,
                    rules: fragment.rule_count(),
                    changes: fragment.changed_pointers(),
                    reserved: fragment.reserved_conflicts(),
                })
                .collect(),
        }
    }

    /// Serializes back to the documented fragment shape, so a file the engine
    /// rewrites always carries its own metadata (ids, labels, order).
    pub fn to_text(&self) -> Result<String> {
        let fragments: Vec<Value> = self
            .fragments
            .iter()
            .map(|fragment| {
                serde_json::json!({
                    "id": fragment.id,
                    "label": fragment.label,
                    "enabled": fragment.enabled,
                    "overlay": fragment.overlay,
                })
            })
            .collect();
        let document = serde_json::json!({ FRAGMENTS_KEY: fragments });
        Ok(format!(
            "{}\n",
            serde_json::to_string_pretty(&document).context("序列化覆写文档")?
        ))
    }
}

impl RuleFragment {
    /// Reserved pointers this fragment tries to change. `serde_json`'s own
    /// pointer lookup takes the rendered form, which is also what the UI shows.
    pub fn reserved_conflicts(&self) -> Vec<String> {
        RESERVED_POINTERS
            .iter()
            .copied()
            .filter(|parts| self.overlay.pointer(&pointer(parts)).is_some())
            .map(pointer)
            .collect()
    }

    /// The field paths this fragment changes, capped for one UI line. A `rules`
    /// array says how many rules it prepends instead of listing them.
    pub fn changed_pointers(&self) -> Vec<String> {
        let mut out = Vec::new();
        collect_pointers(&self.overlay, String::new(), 0, &mut out);
        const CAP: usize = 6;
        if out.len() > CAP {
            let total = out.len();
            out.truncate(CAP);
            out.push(format!("…（共 {total} 处）"));
        }
        out
    }

    /// How many routing rules this fragment prepends when enabled.
    pub fn rule_count(&self) -> usize {
        count_rules(&self.overlay)
    }
}

/// The one fragment parser: strict about shape, because a fragment that fails
/// to say what it targets would otherwise merge into the wrong place.
fn parse_fragment(entry: &Value, path: &Path, index: usize) -> Result<RuleFragment> {
    let at = |key: &str| format!("`/fragments/{index}/{key}`");
    let object = entry
        .as_object()
        .with_context(|| format!("{} 的 `/fragments/{index}` 必须是对象", path.display()))?;
    for key in object.keys() {
        if !matches!(key.as_str(), "id" | "label" | "enabled" | "overlay") {
            bail!(
                "{} 的 {} 不是片段字段（可用：id、label、enabled、overlay）",
                path.display(),
                at(key)
            );
        }
    }
    let id = object
        .get("id")
        .map(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
        .with_context(|| {
            format!(
                "{} 的 {} 必须是字符串（片段要能按 id 开关）",
                path.display(),
                at("id")
            )
        })?;
    if id.trim().is_empty() {
        bail!("{} 的 {} 不能为空", path.display(), at("id"));
    }
    let label = match object.get("label") {
        None => id.clone(),
        Some(value) => value
            .as_str()
            .map(str::to_owned)
            .with_context(|| format!("{} 的 {} 必须是字符串", path.display(), at("label")))?,
    };
    let enabled = match object.get("enabled") {
        None => true,
        Some(value) => value.as_bool().context(format!(
            "{} 的 {} 必须是布尔值",
            path.display(),
            at("enabled")
        ))?,
    };
    let overlay = object
        .get("overlay")
        .and_then(|value| value.as_object())
        .context(format!(
            "{} 的 {} 必须是 JSON 对象（要合并的配置片段）",
            path.display(),
            at("overlay")
        ))?;
    Ok(RuleFragment {
        id,
        label,
        enabled,
        overlay: Value::Object(overlay.clone()),
    })
}

/// Walks an overlay for displayable leaf paths. Bounded: an overlay with
/// hundreds of nodes must not produce hundreds of lines.
fn collect_pointers(value: &Value, path: String, depth: usize, out: &mut Vec<String>) {
    let shown = if path.is_empty() {
        "/".to_owned()
    } else {
        path
    };
    if let Some(object) = value.as_object() {
        if object.is_empty() || depth >= 3 {
            out.push(shown);
            return;
        }
        for (key, child) in object {
            let child_path = if shown == "/" {
                format!("/{key}")
            } else {
                format!("{shown}/{key}")
            };
            // The one array shape worth naming: `rules` is prepended, so the
            // count is the interesting part, not the pointer.
            if key == "rules" && child.is_array() {
                out.push(format!(
                    "{child_path}(+{})",
                    child.as_array().map_or(0, Vec::len)
                ));
                continue;
            }
            collect_pointers(child, child_path, depth + 1, out);
        }
        return;
    }
    if let Some(array) = value.as_array() {
        out.push(format!("{shown}({} 项)", array.len()));
        return;
    }
    out.push(shown);
}

fn count_rules(value: &Value) -> usize {
    match value {
        Value::Object(object) => object
            .iter()
            .map(|(key, child)| {
                if key == "rules"
                    && let Some(rules) = child.as_array()
                {
                    rules.len()
                } else {
                    count_rules(child)
                }
            })
            .sum(),
        Value::Array(array) => array.iter().map(count_rules).sum(),
        _ => 0,
    }
}

/// Writes override text to disk: create the directory, write a sibling
/// temporary file, then rename over the target, so an interrupted write cannot
/// leave the profile with a half-readable override. The mode is owner-only on
/// Unix because an override names internal hosts and rule-set URLs.
pub fn write_file(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| anyhow::anyhow!("创建 {} 失败: {error}", parent.display()))?;
    }
    let temporary = temporary_path(path);
    let written = (|| -> Result<()> {
        use std::io::Write;
        let mut file = std::fs::File::create(&temporary)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, path)?;
        Ok(())
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    written.map_err(|error| anyhow::anyhow!("写入 {} 失败: {error}", path.display()))
}

/// Deletes one profile's override file. A missing file is not an error: "no
/// override" and "override removed" are the same state to the user.
pub fn delete_file(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => bail!("删除 {} 失败: {error}", path.display()),
    }
}

fn temporary_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "override".to_owned());
    path.with_file_name(format!("{name}.tmp"))
}

impl fmt::Display for ProfileOverride {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}（{}/{} 个片段启用，{} 字节）",
            self.path.display(),
            self.enabled().len(),
            self.fragments.len(),
            self.bytes
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn path() -> PathBuf {
        PathBuf::from("/data/overrides/deadbeef.json")
    }

    fn document(fragments: Vec<Value>) -> String {
        serde_json::to_string(&json!({ FRAGMENTS_KEY: fragments })).expect("valid JSON")
    }

    fn fragment(id: &str, overlay: Value) -> Value {
        json!({"id": id, "label": id, "enabled": true, "overlay": overlay})
    }

    #[test]
    fn the_shared_merge_table_holds_here_too() {
        // The server and the client read the same rows: ADR-0021 documents one
        // merge, so a semantic change cannot land for one side only.
        assert_eq!(json_merge::merge_semantics_failures(), Vec::<&str>::new());
    }

    #[test]
    fn a_missing_or_blank_file_means_no_override() {
        let dir = tempfile::tempdir().expect("a data directory");
        let path = dir.path().join("overrides/none.json");
        assert!(
            ProfileOverride::load(&path)
                .expect("a missing file is not an error")
                .is_none()
        );
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("directory");
        std::fs::write(&path, "   \n").expect("a blank file");
        assert!(
            ProfileOverride::load(&path)
                .expect("a blank file is not an error")
                .is_none(),
            "an empty file must not report itself as an override that changes nothing"
        );
    }

    #[test]
    fn a_malformed_override_is_diagnosed_with_a_field_path() {
        let error = ProfileOverride::parse("{ \"route\": {\"rules\": }", &path())
            .expect_err("broken JSON must be refused");
        let message = error.to_string();
        assert!(message.contains("不是有效的 JSON"), "{message}");
        assert!(message.contains("`/`"), "the root path is named: {message}");
        assert!(
            message.contains("deadbeef.json"),
            "the file is named: {message}"
        );

        let error = ProfileOverride::parse(r#"{"fragments": {}}"#, &path())
            .expect_err("a mapping is not a fragment list");
        assert!(error.to_string().contains("`/fragments`"), "{error}");

        let error = ProfileOverride::parse(
            &document(vec![json!({"id": "a", "overlay": ["not", "an", "object"]})]),
            &path(),
        )
        .expect_err("an overlay must be an object");
        assert!(
            error.to_string().contains("/fragments/0/overlay"),
            "the message must name the offending field: {error}"
        );

        let error = ProfileOverride::parse(
            &document(vec![json!({"id": "a", "overlay": {}, "rule": []})]),
            &path(),
        )
        .expect_err("a misspelled fragment key must be reported, not ignored");
        assert!(
            error.to_string().contains("/fragments/0/rule"),
            "the message must name the unknown field: {error}"
        );

        let error = ProfileOverride::parse(
            &document(vec![json!({"id": "a", "overlay": {}, "enabled": "yes"})]),
            &path(),
        )
        .expect_err("a string is not a switch");
        assert!(
            error.to_string().contains("/fragments/0/enabled"),
            "{error}"
        );
    }

    #[test]
    fn duplicate_fragment_ids_are_refused_because_toggles_address_by_id() {
        let error = ProfileOverride::parse(
            &document(vec![
                fragment("same", json!({})),
                fragment("same", json!({})),
            ]),
            &path(),
        )
        .expect_err("two fragments with one id cannot be toggled apart");
        assert!(error.to_string().contains("`same` 重复"), "{error}");
    }

    #[test]
    fn an_enabled_fragment_lands_in_the_config_and_a_disabled_one_does_not() {
        let text = document(vec![
            fragment("dns", json!({"dns": {"servers": ["223.5.5.5"]}})),
            fragment(
                "blocked",
                json!({"route": {"rules": [{"action": "direct", "ip_cidr": ["10.0.0.0/8"]}]}}),
            ),
        ]);
        let parsed = ProfileOverride::parse(&text, &path()).expect("a valid document");
        let mut base: Value =
            json!({"route": {"rules": [{"action": "proxy"}]}, "log": {"level": "info"}});
        parsed.apply(&mut base);
        assert_eq!(
            base["route"]["rules"],
            json!([
                {"action": "direct", "ip_cidr": ["10.0.0.0/8"]},
                {"action": "proxy"}
            ]),
            "rules prepend and the profile's own verdict survives below them"
        );
        assert_eq!(base["log"]["level"], "info", "the merge must not drop keys");
        assert_eq!(base["dns"]["servers"], json!(["223.5.5.5"]));

        let mut second = parsed.clone();
        second.toggle("blocked", None).expect("toggle by id");
        let mut base: Value = json!({"route": {"rules": [{"action": "proxy"}]}});
        assert_eq!(second.apply(&mut base), Vec::<String>::new());
        assert_eq!(
            base["route"]["rules"],
            json!([{"action": "proxy"}]),
            "the disabled fragment contributes no rule"
        );
        assert_eq!(
            base["dns"]["servers"],
            json!(["223.5.5.5"]),
            "and disabling one fragment must not switch the other one off"
        );

        // Toggling back is a round trip through the file format, not a rewrite
        // that loses the other fragment's metadata.
        let text_after = second.to_text().expect("serializable");
        let reloaded = ProfileOverride::parse(&text_after, &path()).expect("round trip");
        assert_eq!(
            reloaded
                .fragments
                .iter()
                .map(|fragment| (fragment.id.as_str(), fragment.enabled))
                .collect::<Vec<_>>(),
            vec![("dns", true), ("blocked", false)]
        );
    }

    #[test]
    fn toggling_an_unknown_id_says_what_exists_instead_of_failing_quietly() {
        let mut parsed =
            ProfileOverride::parse(&document(vec![fragment("mine", json!({}))]), &path())
                .expect("a valid document");
        let error = parsed
            .toggle("theirs", None)
            .expect_err("an unknown id must not be a no-op");
        let message = error.to_string();
        assert!(message.contains("`theirs`"), "{message}");
        assert!(message.contains("现有片段：mine"), "{message}");
    }

    #[test]
    fn the_first_listed_fragment_wins_the_rule_order_and_shared_keys() {
        let text = document(vec![
            fragment(
                "first",
                json!({"route": {"rules": [{"tag": "first"}], "final": "direct"},
                       "log": {"level": "warning"}}),
            ),
            fragment(
                "second",
                json!({"route": {"rules": [{"tag": "second"}], "final": "proxy"},
                       "log": {"level": "info"}}),
            ),
        ]);
        let parsed = ProfileOverride::parse(&text, &path()).expect("a valid document");
        let mut base: Value = json!({"route": {"rules": [{"tag": "generated"}]}});
        parsed.apply(&mut base);
        assert_eq!(
            base["route"]["rules"],
            json!([{"tag": "first"}, {"tag": "second"}, {"tag": "generated"}]),
            "the panel lists rules top-down, so the first fragment must be first"
        );
        assert_eq!(base["route"]["final"], "direct");
        assert_eq!(base["log"]["level"], "warning");
    }

    #[test]
    fn a_reserved_field_is_reported_so_the_caller_can_write_it_back() {
        let text = document(vec![fragment(
            "take-over",
            json!({
                "experimental": {"clash_api": {"external_controller": "0.0.0.0:9090", "secret": "attacker"}},
                "route": {"auto_detect_interface": false},
                "inbounds": [{"type": "mixed", "listen_port": 1080}]
            }),
        )]);
        let parsed = ProfileOverride::parse(&text, &path()).expect("a valid document");
        let mut base: Value = json!({
            "experimental": {"clash_api": {"external_controller": "127.0.0.1:5"}},
            "route": {"auto_detect_interface": true}
        });
        let conflicts = parsed.apply(&mut base);
        assert_eq!(
            conflicts,
            vec![
                "/experimental/clash_api/external_controller",
                "/experimental/clash_api/secret",
                "/inbounds",
                "/route/auto_detect_interface",
            ],
            "every reserved pointer the file touched is reported: {conflicts:?}"
        );
        assert_eq!(
            parsed.reserved_conflicts(),
            conflicts,
            "the same report is available before a start, for the panel"
        );
        // The merge did change the document — reporting is only half the guard;
        // `runtime_config` writes the reserved fields back, which the core test
        // asserts end to end.
        assert_eq!(base["experimental"]["clash_api"]["secret"], "attacker");
        assert_eq!(base["route"]["auto_detect_interface"], json!(false));

        let innocent = ProfileOverride::parse(
            &document(vec![fragment("dns", json!({"dns": {"tag": "override"}}))]),
            &path(),
        )
        .expect("a valid document");
        assert_eq!(innocent.reserved_conflicts(), Vec::<String>::new());
    }

    #[test]
    fn a_disabled_fragment_cannot_report_a_conflict_it_will_not_cause() {
        let text = document(vec![
            json!({"id": "quiet", "enabled": false, "overlay": {"inbounds": []}}),
            fragment("loud", json!({"route": {"auto_detect_interface": false}})),
        ]);
        let parsed = ProfileOverride::parse(&text, &path()).expect("a valid document");
        assert_eq!(
            parsed.reserved_conflicts(),
            vec!["/route/auto_detect_interface"]
        );
    }

    #[test]
    fn a_bare_object_is_one_implicit_fragment_that_cannot_be_toggled_apart() {
        let parsed = ProfileOverride::parse(r#"{"dns": {"servers": ["1.1.1.1"]}}"#, &path())
            .expect("the hand-written form");
        assert!(parsed.implicit);
        assert_eq!(parsed.fragments.len(), 1);
        assert!(parsed.fragments[0].enabled);
        let mut base = json!({});
        assert_eq!(parsed.apply(&mut base), Vec::<String>::new());
        assert_eq!(base["dns"]["servers"], json!(["1.1.1.1"]));
        let error = parsed
            .clone()
            .toggle(IMPLICIT_FRAGMENT_ID, Some(false))
            .expect_err("there is no fragment list to rewrite");
        assert!(error.to_string().contains("手写整份覆写"), "{error}");
    }

    #[test]
    fn an_empty_fragment_list_is_an_override_that_changes_nothing() {
        let parsed = ProfileOverride::parse(r#"{"fragments": []}"#, &path())
            .expect("an explicit empty list");
        assert!(!parsed.implicit, "an empty list is not the bare form");
        assert!(parsed.enabled().is_empty());
        let mut base = json!({"route": {"rules": [1]}});
        assert_eq!(parsed.apply(&mut base), Vec::<String>::new());
        assert_eq!(base["route"]["rules"], json!([1]));
    }

    #[test]
    fn the_change_summary_names_rules_by_count_not_by_dump() {
        let parsed = ProfileOverride::parse(
            &document(vec![fragment(
                "ai",
                json!({
                    "route": {"rules": [{"action": "direct"}, {"action": "direct"}], "final": "direct"},
                    "dns": {"servers": ["1.1.1.1", "8.8.8.8", "9.9.9.9"]}
                }),
            )]),
            &path(),
        )
        .expect("a valid document");
        let changes = parsed.changed_pointers();
        assert!(
            changes.contains(&"/route/rules(+2)".to_owned()),
            "a prepended rule list reads as a count: {changes:?}"
        );
        assert!(
            changes.contains(&"/dns/servers(3 项)".to_owned()),
            "a replaced array says how many entries: {changes:?}"
        );
        assert!(changes.contains(&"/route/final".to_owned()), "{changes:?}");
        assert_eq!(parsed.fragments[0].rule_count(), 2);
    }

    #[test]
    fn writing_and_deleting_one_file_is_the_whole_storage_contract() {
        let dir = tempfile::tempdir().expect("a data directory");
        let path = dir.path().join("overrides/profile.json");
        assert!(ProfileOverride::load(&path).expect("absent").is_none());
        let text = document(vec![fragment("a", json!({"log": {"level": "debug"}}))]);
        write_file(&path, &text).expect("the file writes, creating its directory");
        let loaded = ProfileOverride::load(&path)
            .expect("readable")
            .expect("present");
        assert_eq!(loaded.bytes, text.len() as u64);
        assert_eq!(loaded.path, path);
        assert_eq!(
            loaded
                .fragments
                .first()
                .map(|fragment| fragment.id.as_str()),
            Some("a")
        );
        delete_file(&path).expect("deleting works");
        assert!(!path.exists());
        delete_file(&path).expect("deleting an absent file is not an error");
    }

    #[cfg(unix)]
    #[test]
    fn an_override_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("a data directory");
        let path = dir.path().join("overrides/private.json");
        write_file(&path, r#"{"fragments": []}"#).expect("written");
        let mode = std::fs::metadata(&path)
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "an override can name internal hosts");
    }

    /// The rename step is what makes a half-written file impossible to read,
    /// so the failure has to leave the previous override alone. A directory
    /// sitting on the temporary name makes the write fail for real.
    #[test]
    fn a_failed_write_leaves_the_previous_override_in_place() {
        let dir = tempfile::tempdir().expect("a data directory");
        let path = dir.path().join("overrides/one.json");
        let previous = document(vec![fragment("a", json!({"log": {"level": "info"}}))]);
        write_file(&path, &previous).expect("the first write creates the directory");
        std::fs::create_dir(temporary_path(&path)).expect("the temporary name is taken");
        let text = document(vec![fragment("b", json!({"log": {"level": "debug"}}))]);
        write_file(&path, &text).expect_err("the rename cannot replace a directory");
        assert_eq!(
            std::fs::read_to_string(&path).expect("the old file is intact"),
            previous,
            "a failed write must not truncate the override in place"
        );
    }
}
