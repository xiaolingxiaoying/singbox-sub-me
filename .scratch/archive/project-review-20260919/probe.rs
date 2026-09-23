use client_core::{settings::profile_cache_path, subscription::{normalize_url, bare_sing_box_fallback_url}};
use std::path::Path;
fn main() {
    let base = Path::new("audit-fixture");
    let a = profile_cache_path(base, "a b");
    let b = profile_cache_path(base, "a_b");
    assert_eq!(a, b);
    println!("CACHE_COLLISION: a b and a_b -> {}", a.display());
    let input = "https://example.test/sub/test-credential/sing-box.json";
    let normalized = normalize_url(input);
    let fallback = bare_sing_box_fallback_url(&normalized);
    assert!(fallback.is_none());
    println!("LEGACY_FALLBACK: normalized={} fallback={:?}", normalized, fallback);
    let reserved = profile_cache_path(base, "active-config");
    assert_eq!(reserved, base.join("cache/active-config.json"));
    println!("RUNTIME_CONFIG_COLLISION: {}", reserved.display());
}
