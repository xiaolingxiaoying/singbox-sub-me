//! This target must also run WITHOUT test-signing: fixture trust must never
//! silently leak into ordinary production builds.
#[cfg(not(feature = "test-signing"))]
#[test]
fn ordinary_build_rejects_public_development_signatures() {
    use sbctl::release::*;
    let seed = parse_seed_hex(include_str!("../scripts/dev-signing-key.hex")).unwrap();
    let artifact = ReleaseArtifact {
        version: "1.12.0".into(),
        sha256: "a".repeat(64),
        url: None,
    };
    let mut manifest = ReleaseManifest {
        schema: 1,
        sbctl: artifact.clone(),
        sing_box: artifact,
        sing_box_compatibility: vec![CompatibilityRange {
            min: Some("1.12.0".into()),
            max: None,
        }],
        signature: None,
    };
    manifest.signature = Some(sign_manifest(&manifest, &seed).unwrap());
    assert!(verify_trusted(&manifest).is_err());
}
