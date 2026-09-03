//! Signed, fixed-version release manifests.
//!
//! Every manifest carries a versioned schema, a fixed sbctl/sing-box version
//! with artifact URL and SHA-256 digest, a sing-box compatibility matrix, and
//! an Ed25519 signature over the canonical JSON encoding of every field except
//! `signature`. The signature is verified before any URL or digest is trusted;
//! the built-in first-release public key is the trust anchor for both the Rust
//! update logic and the bootstrap install script.

use std::fs;
use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The only schema version the current client accepts.
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

/// The first-release Ed25519 verification key, hex-encoded. The matching
/// private key is held by the release maintainer; a development keypair lives
/// in `scripts/dev-signing-key.hex` and the same public key is embedded in the
/// bootstrap install script.
pub const FIRST_RELEASE_PUBLIC_KEY: [u8; 32] = [
    0x24, 0x7F, 0x88, 0xE1, 0x63, 0x24, 0x29, 0x86, 0xB7, 0x10, 0x7E, 0xB7, 0x04, 0xA9, 0x83, 0xE1,
    0x21, 0x86, 0xD2, 0x69, 0x7A, 0x39, 0x27, 0xB7, 0xE6, 0xB4, 0x2E, 0xC2, 0xB3, 0x64, 0x27, 0x2B,
];

/// Floating, unsignable version references that must never be trusted.
const FLOATING_VERSIONS: &[&str] = &["latest", "main", "master"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseManifest {
    pub schema: u32,
    pub sbctl: ReleaseArtifact,
    pub sing_box: ReleaseArtifact,
    #[serde(default)]
    pub sing_box_compatibility: Vec<CompatibilityRange>,
    /// Standard-Base64 Ed25519 signature over the canonical JSON of every
    /// field except this one. Never included in the canonical payload.
    #[serde(skip_serializing, default)]
    pub signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseArtifact {
    pub version: String,
    pub sha256: String,
    #[serde(default)]
    pub url: Option<String>,
}

/// An inclusive sing-box version range. A candidate is compatible when it lies
/// within at least one declared range; the matrix must never be empty.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompatibilityRange {
    #[serde(default)]
    pub min: Option<String>,
    #[serde(default)]
    pub max: Option<String>,
}

#[derive(Debug, Error)]
pub enum ReleaseError {
    #[error("could not read the pinned release manifest: {0}")]
    ManifestRead(#[from] std::io::Error),
    #[error("could not parse the pinned release manifest: {0}")]
    ManifestParse(#[from] serde_json::Error),
    #[error("pinned release manifest schema version {0} is not supported")]
    SchemaMismatch(u32),
    #[error("pinned release manifest is unsigned")]
    UnsignedManifest,
    #[error("pinned release manifest signature is invalid")]
    InvalidSignature,
    #[error("pinned release manifest has an invalid SHA-256 for {0}")]
    InvalidDigest(&'static str),
    #[error("pinned release manifest references the unsupported version {0:?} for {1}")]
    FloatingVersion(&'static str, String),
    #[error("pinned release manifest version {0:?} for {1} is not a semantic version")]
    MalformedVersion(String, &'static str),
    #[error("pinned release manifest declares an empty sing-box compatibility matrix")]
    EmptyCompatibilityMatrix,
    #[error(
        "pinned release manifest declares sing-box {version} which is outside the compatibility matrix {ranges}"
    )]
    CompatibilityMismatch { version: String, ranges: String },
    #[error("could not serialize the release manifest: {0}")]
    Serialize(serde_json::Error),
    #[error("invalid Ed25519 signing key seed: {0}")]
    InvalidKeySeed(String),
}

/// Reads and fully validates a pinned release manifest. Signature verification
/// happens before any digest, URL, or compatibility claim is trusted.
pub fn verify_manifest(path: &Path) -> Result<ReleaseManifest, ReleaseError> {
    let manifest: ReleaseManifest = serde_json::from_slice(&fs::read(path)?)?;
    verify_trusted(&manifest)?;
    Ok(manifest)
}

/// Validates the full trust chain of an already-parsed manifest: schema
/// version, fixed versions, signature, digest format, and the compatibility
/// matrix. The signature is verified before any digest, URL, or compatibility
/// claim is trusted.
pub fn verify_trusted(manifest: &ReleaseManifest) -> Result<(), ReleaseError> {
    validate_manifest_fields(manifest)?;
    verify_signature(manifest)
}

/// Validates every manifest field except the signature. The signer uses this
/// so it can never produce a signed manifest for a floating version, a wrong
/// schema, an empty compatibility matrix, or a malformed matrix boundary.
pub fn validate_manifest_fields(manifest: &ReleaseManifest) -> Result<(), ReleaseError> {
    if manifest.schema != MANIFEST_SCHEMA_VERSION {
        return Err(ReleaseError::SchemaMismatch(manifest.schema));
    }
    reject_floating_version(&manifest.sbctl.version, "sbctl")?;
    reject_floating_version(&manifest.sing_box.version, "sing-box")?;
    validate_digest("sbctl", &manifest.sbctl.sha256)?;
    validate_digest("sing-box", &manifest.sing_box.sha256)?;
    check_compatibility_matrix(manifest)
}

/// The canonical JSON payload that the signature covers: every field except
/// `signature`, compact, with object keys in sorted order. Both the Rust
/// update logic and the bootstrap install script canonicalize this way (the
/// script uses `jq -S -c 'del(.signature)'`), and the exact encoding is pinned
/// by `canonical_bytes_are_stable`.
pub fn canonical_bytes(manifest: &ReleaseManifest) -> Result<Vec<u8>, ReleaseError> {
    let value = serde_json::to_value(manifest)?;
    Ok(serde_json::to_vec(&value)?)
}

fn verify_signature(manifest: &ReleaseManifest) -> Result<(), ReleaseError> {
    let encoded = manifest
        .signature
        .as_deref()
        .ok_or(ReleaseError::UnsignedManifest)?;
    let signature_bytes = STANDARD
        .decode(encoded)
        .map_err(|_| ReleaseError::InvalidSignature)?;
    let signature =
        Signature::from_slice(&signature_bytes).map_err(|_| ReleaseError::InvalidSignature)?;
    let key = VerifyingKey::from_bytes(&FIRST_RELEASE_PUBLIC_KEY)
        .map_err(|_| ReleaseError::InvalidSignature)?;
    let canonical = canonical_bytes(manifest)?;
    key.verify_strict(&canonical, &signature)
        .map_err(|_| ReleaseError::InvalidSignature)
}

/// Serializes a manifest for disk, re-inserting the `signature` field that the
/// canonical encoding deliberately omits. The signature is excluded from the
/// signed bytes but must still be present in the published manifest.
pub fn manifest_json_with_signature(manifest: &ReleaseManifest) -> Result<Vec<u8>, ReleaseError> {
    let mut value = serde_json::to_value(manifest)?;
    if let Some(signature) = &manifest.signature {
        value
            .as_object_mut()
            .expect("manifest serializes to a JSON object")
            .insert(
                "signature".to_owned(),
                serde_json::Value::String(signature.clone()),
            );
    }
    Ok(serde_json::to_vec_pretty(&value)?)
}

/// Signs an unsigned manifest file with a seed from a key file and writes the
/// signed manifest, validating all manifest fields first so the signer can
/// never mint a signature for a floating version, wrong schema, or empty
/// compatibility matrix. Used by `sbctl release sign` and by the release
/// manifest generator.
pub fn sign_manifest_file(
    manifest_path: &Path,
    seed_path: &Path,
    output_path: &Path,
) -> Result<(), ReleaseError> {
    let contents = fs::read_to_string(manifest_path)?;
    let seed = fs::read_to_string(seed_path)
        .map_err(|error| ReleaseError::InvalidKeySeed(error.to_string()))
        .and_then(|contents| parse_seed_hex(&contents))?;
    let mut manifest: ReleaseManifest = serde_json::from_str(&contents)?;
    validate_manifest_fields(&manifest)?;
    manifest.signature = Some(sign_manifest(&manifest, &seed)?);
    let signed = manifest_json_with_signature(&manifest)?;
    fs::write(output_path, signed)?;
    Ok(())
}

/// Writes a fresh signing key seed to `path` with 0600 permissions so the
/// secret never appears on a terminal or in logs.
pub fn write_secret_file(path: &Path, secret: &[u8; 32]) -> Result<(), ReleaseError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true).mode(0o600);
        let mut file = options.open(path)?;
        use std::io::Write;
        file.write_all(&format_hex(secret).into_bytes())?;
        file.sync_all()?;
    }
    #[cfg(not(unix))]
    {
        use std::io::Write;
        fs::File::create(path)
            .and_then(|mut file| file.write_all(&format_hex(secret).into_bytes()))?;
    }
    Ok(())
}

fn format_hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Signs the canonical payload of `manifest` (the `signature` field is never
/// part of the signed bytes) with an Ed25519 signing key seed and returns the
/// standard-Base64 signature.
pub fn sign_manifest(
    manifest: &ReleaseManifest,
    secret_seed: &[u8; 32],
) -> Result<String, ReleaseError> {
    let signing_key = SigningKey::from_bytes(secret_seed);
    let canonical = canonical_bytes(manifest)?;
    let signature = signing_key.sign(&canonical);
    Ok(STANDARD.encode(signature.to_bytes()))
}

/// Parses an Ed25519 signing key seed from hex text. Comment lines beginning
/// with `#` and blank lines are ignored, so the development seed file in
/// `scripts/dev-signing-key.hex` can be passed directly.
pub fn parse_seed_hex(contents: &str) -> Result<[u8; 32], ReleaseError> {
    let hex = contents
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .ok_or_else(|| ReleaseError::InvalidKeySeed("no seed line found".to_owned()))?;
    let hex = hex.strip_prefix("0x").unwrap_or(hex);
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ReleaseError::InvalidKeySeed(
            "expected exactly 64 hex digits".to_owned(),
        ));
    }
    let mut seed = [0u8; 32];
    for (index, slot) in seed.iter_mut().enumerate() {
        let byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16)
            .map_err(|_| ReleaseError::InvalidKeySeed("non-hex digit".to_owned()))?;
        *slot = byte;
    }
    Ok(seed)
}

/// Returns the public key bytes for a signing key seed, as the signer-side
/// mirror of `FIRST_RELEASE_PUBLIC_KEY`.
pub fn public_key(secret_seed: &[u8; 32]) -> [u8; 32] {
    SigningKey::from_bytes(secret_seed)
        .verifying_key()
        .to_bytes()
}

/// A new random keypair as (public, secret). `release keygen` prints both so a
/// maintainer can rotate the embedded key; the development keypair is stored in
/// `scripts/dev-signing-key.hex`.
pub fn generate_keypair() -> ([u8; 32], [u8; 32]) {
    let mut secret = [0u8; 32];
    getrandom::fill(&mut secret).expect("operating system provides randomness");
    (public_key(&secret), secret)
}

/// PKCS#8 SubjectPublicKeyInfo PEM for an Ed25519 public key, used by the
/// bootstrap install script so it can verify the same signature before trusting
/// any URL or digest.
pub fn public_key_pem(public: &[u8; 32]) -> String {
    // DER SubjectPublicKeyInfo for id-Ed25519 (1.3.101.112).
    let mut der = vec![
        0x30, 0x2A, 0x30, 0x05, 0x06, 0x03, 0x2B, 0x65, 0x70, 0x03, 0x21, 0x00,
    ];
    der.extend_from_slice(public);
    let encoded = STANDARD.encode(der);
    let mut lines = Vec::new();
    for chunk in encoded.as_bytes().chunks(64) {
        lines.push(String::from_utf8_lossy(chunk).to_string());
    }
    format!(
        "-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----\n",
        lines.join("\n")
    )
}

fn reject_floating_version(version: &str, name: &'static str) -> Result<(), ReleaseError> {
    if FLOATING_VERSIONS.contains(&version) {
        return Err(ReleaseError::FloatingVersion(name, version.to_owned()));
    }
    Ok(())
}

fn validate_digest(name: &'static str, digest: &str) -> Result<(), ReleaseError> {
    (digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then_some(())
        .ok_or(ReleaseError::InvalidDigest(name))
}
fn check_compatibility_matrix(manifest: &ReleaseManifest) -> Result<(), ReleaseError> {
    if manifest.sing_box_compatibility.is_empty() {
        return Err(ReleaseError::EmptyCompatibilityMatrix);
    }
    let version = parse_version(&manifest.sing_box.version, "sing-box")?;
    // A malformed min/max boundary must fail closed rather than acting as an
    // unbounded edge, so parse every boundary before testing membership.
    let mut parsed = Vec::new();
    for range in &manifest.sing_box_compatibility {
        let min = range
            .min
            .as_deref()
            .map(|value| parse_version(value, "sing-box compatibility min"))
            .transpose()?;
        let max = range
            .max
            .as_deref()
            .map(|value| parse_version(value, "sing-box compatibility max"))
            .transpose()?;
        parsed.push((min, max));
    }
    let ranges = manifest
        .sing_box_compatibility
        .iter()
        .map(range_display)
        .collect::<Vec<_>>()
        .join(", ");
    let compatible = parsed.iter().any(|(min, max)| {
        !matches!(min, Some(min) if version < *min) && !matches!(max, Some(max) if version > *max)
    });
    if compatible {
        Ok(())
    } else {
        Err(ReleaseError::CompatibilityMismatch {
            version: manifest.sing_box.version.clone(),
            ranges,
        })
    }
}

fn range_display(range: &CompatibilityRange) -> String {
    let min = range.min.as_deref().unwrap_or("*");
    let max = range.max.as_deref().unwrap_or("*");
    format!("{min}..={max}")
}

/// Parses a `major.minor[.patch]` semantic version. Leading `v` is accepted;
/// pre-release suffixes and non-numeric components are rejected so a
/// moving or malformed version can never satisfy the matrix.
pub fn parse_version(value: &str, name: &'static str) -> Result<[u64; 3], ReleaseError> {
    let value = value.strip_prefix('v').unwrap_or(value);
    let mut parts = value.split('.');
    let mut version = [0u64; 3];
    for slot in version.iter_mut().take(3) {
        let Some(part) = parts.next() else { break };
        if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(ReleaseError::MalformedVersion(value.to_owned(), name));
        }
        let Ok(number) = part.parse::<u64>() else {
            return Err(ReleaseError::MalformedVersion(value.to_owned(), name));
        };
        *slot = number;
    }
    if parts.next().is_some() {
        return Err(ReleaseError::MalformedVersion(value.to_owned(), name));
    }
    Ok(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SBCTL_VERSION: &str = "0.1.14";
    const SING_BOX_VERSION: &str = "1.12.0";

    fn test_manifest() -> ReleaseManifest {
        ReleaseManifest {
            schema: MANIFEST_SCHEMA_VERSION,
            sbctl: ReleaseArtifact {
                version: SBCTL_VERSION.to_owned(),
                url: Some("https://example.test/sbctl".to_owned()),
                sha256: "a".repeat(64),
            },
            sing_box: ReleaseArtifact {
                version: SING_BOX_VERSION.to_owned(),
                url: Some("https://example.test/sing-box".to_owned()),
                sha256: "b".repeat(64),
            },
            sing_box_compatibility: vec![CompatibilityRange {
                min: Some("1.10.0".to_owned()),
                max: Some("1.12.9".to_owned()),
            }],
            signature: None,
        }
    }

    fn signed(manifest: &mut ReleaseManifest, secret: &[u8; 32]) {
        manifest.signature = Some(sign_manifest(manifest, secret).expect("signature is produced"));
    }

    // The development signing seed that matches FIRST_RELEASE_PUBLIC_KEY.
    const SECRET: [u8; 32] = [
        0x62, 0xac, 0x3d, 0x58, 0x01, 0xb5, 0x2a, 0x11, 0xda, 0x92, 0x3e, 0xbb, 0xbc, 0xcb, 0x88,
        0xa6, 0x7f, 0x15, 0x4b, 0xde, 0x43, 0x39, 0x59, 0x6f, 0x05, 0x1e, 0xe2, 0x73, 0x2e, 0x9a,
        0x36, 0x5f,
    ];

    #[test]
    fn canonical_bytes_exclude_the_signature_and_pin_the_exact_encoding() {
        let mut manifest = test_manifest();
        signed(&mut manifest, &SECRET);
        let canonical = canonical_bytes(&manifest).expect("canonical bytes are produced");
        assert_eq!(
            String::from_utf8(canonical).expect("canonical bytes are UTF-8"),
            "{\"sbctl\":{\"sha256\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\",\"url\":\"https://example.test/sbctl\",\"version\":\"0.1.14\"},\"schema\":1,\"sing_box\":{\"sha256\":\"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\",\"url\":\"https://example.test/sing-box\",\"version\":\"1.12.0\"},\"sing_box_compatibility\":[{\"max\":\"1.12.9\",\"min\":\"1.10.0\"}]}"
        );
        assert!(
            !canonical_bytes(&manifest)
                .expect("canonical")
                .windows(10)
                .any(|w| w == b"signature")
        );
    }

    #[test]
    fn a_manifest_signed_with_the_built_in_key_verifies() {
        let mut manifest = test_manifest();
        signed(&mut manifest, &SECRET);
        assert!(verify_signature(&manifest).is_ok());
        assert!(verify_trusted(&manifest).is_ok());
    }

    #[test]
    fn an_unsigned_manifest_is_rejected_before_urls_or_digests_are_considered() {
        let manifest = test_manifest();
        assert!(matches!(
            verify_signature(&manifest),
            Err(ReleaseError::UnsignedManifest)
        ));
    }

    #[test]
    fn a_signature_from_a_different_key_is_rejected() {
        let mut manifest = test_manifest();
        let mut wrong_secret = SECRET;
        wrong_secret[0] ^= 0xff;
        signed(&mut manifest, &wrong_secret);
        assert!(matches!(
            verify_signature(&manifest),
            Err(ReleaseError::InvalidSignature)
        ));
    }

    #[test]
    fn corrupted_signature_or_wrong_schema_or_digest_are_rejected() {
        let mut manifest = test_manifest();
        signed(&mut manifest, &SECRET);
        manifest.signature = Some("not-base64!!".to_owned());
        assert!(matches!(
            verify_signature(&manifest),
            Err(ReleaseError::InvalidSignature)
        ));

        let mut manifest = test_manifest();
        signed(&mut manifest, &SECRET);
        manifest.schema = 2;
        assert!(matches!(
            verify_trusted(&manifest),
            Err(ReleaseError::SchemaMismatch(2))
        ));

        let mut manifest = test_manifest();
        manifest.sbctl.sha256 = "not-hex".to_owned();
        signed(&mut manifest, &SECRET);
        assert!(matches!(
            verify_trusted(&manifest),
            Err(ReleaseError::InvalidDigest("sbctl"))
        ));
    }

    #[test]
    fn signature_tampering_after_signing_is_rejected() {
        let mut manifest = test_manifest();
        signed(&mut manifest, &SECRET);
        let mut tampered = test_manifest();
        tampered.sbctl.url = Some("https://evil.example.test/sbctl".to_owned());
        tampered.signature = manifest.signature.clone();
        assert!(matches!(
            verify_signature(&tampered),
            Err(ReleaseError::InvalidSignature)
        ));
    }

    #[test]
    fn floating_versions_are_rejected() {
        for version in ["latest", "main", "master"] {
            let mut manifest = test_manifest();
            manifest.sing_box.version = version.to_owned();
            signed(&mut manifest, &SECRET);
            assert!(matches!(
                verify_trusted(&manifest),
                Err(ReleaseError::FloatingVersion("sing-box", _))
            ));
        }
    }

    #[test]
    fn empty_compatibility_matrix_is_rejected() {
        let mut manifest = test_manifest();
        manifest.sing_box_compatibility = vec![];
        signed(&mut manifest, &SECRET);
        assert!(matches!(
            verify_trusted(&manifest),
            Err(ReleaseError::EmptyCompatibilityMatrix)
        ));
    }

    #[test]
    fn compatibility_matrix_rejects_out_of_range_versions() {
        let mut manifest = test_manifest();
        manifest.sing_box.version = "1.13.0".to_owned();
        signed(&mut manifest, &SECRET);
        assert!(matches!(
            verify_trusted(&manifest),
            Err(ReleaseError::CompatibilityMismatch { .. })
        ));

        let mut manifest = test_manifest();
        manifest.sing_box.version = "1.9.9".to_owned();
        signed(&mut manifest, &SECRET);
        assert!(matches!(
            verify_trusted(&manifest),
            Err(ReleaseError::CompatibilityMismatch { .. })
        ));
    }

    #[test]
    fn compatibility_matrix_accepts_boundary_and_open_ranges() {
        let mut manifest = test_manifest();
        manifest.sing_box.version = "1.10.0".to_owned();
        signed(&mut manifest, &SECRET);
        assert!(verify_trusted(&manifest).is_ok());

        manifest.sing_box.version = "1.12.9".to_owned();
        signed(&mut manifest, &SECRET);
        assert!(verify_trusted(&manifest).is_ok());

        let mut manifest = test_manifest();
        manifest.sing_box_compatibility = vec![CompatibilityRange {
            min: None,
            max: Some("1.12.9".to_owned()),
        }];
        manifest.sing_box.version = "1.0.0".to_owned();
        signed(&mut manifest, &SECRET);
        assert!(verify_trusted(&manifest).is_ok());
    }

    #[test]
    fn compatibility_matrix_rejects_malformed_boundaries_fail_closed() {
        for min in ["1.x", "latest", "1..2"] {
            let mut manifest = test_manifest();
            manifest.sing_box_compatibility = vec![CompatibilityRange {
                min: Some(min.to_owned()),
                max: None,
            }];
            signed(&mut manifest, &SECRET);
            assert!(
                matches!(
                    verify_trusted(&manifest),
                    Err(ReleaseError::MalformedVersion(
                        _,
                        "sing-box compatibility min"
                    ))
                ),
                "malformed min {min:?} must fail closed"
            );
        }
        let mut manifest = test_manifest();
        manifest.sing_box_compatibility = vec![CompatibilityRange {
            min: None,
            max: Some("1.12.9-beta".to_owned()),
        }];
        signed(&mut manifest, &SECRET);
        assert!(matches!(
            verify_trusted(&manifest),
            Err(ReleaseError::MalformedVersion(
                _,
                "sing-box compatibility max"
            ))
        ));
    }

    #[test]
    fn malformed_versions_are_rejected() {
        for version in ["1.2.3.4", "1.x", "1..2", "", "1.2.3-beta"] {
            assert!(parse_version(version, "sing-box").is_err());
        }
        assert_eq!(
            parse_version("v1.12", "sing-box").expect("version parses"),
            [1, 12, 0]
        );
    }

    #[test]
    fn standard_base64_signatures_round_trip() {
        let mut manifest = test_manifest();
        signed(&mut manifest, &SECRET);
        let encoded = manifest.signature.as_deref().expect("signature is present");
        assert!(!encoded.contains('_'));
        assert!(!encoded.contains('-'));
        let decoded = STANDARD
            .decode(encoded)
            .expect("signature is standard Base64");
        assert_eq!(decoded.len(), 64);
    }

    #[test]
    fn keygen_produces_a_verifying_public_key_and_pem() {
        let (public, secret) = generate_keypair();
        assert_eq!(public_key(&secret), public);
        let pem = public_key_pem(&public);
        assert!(pem.starts_with("-----BEGIN PUBLIC KEY-----\n"));
        assert!(pem.contains("-----END PUBLIC KEY-----\n"));
        let decoded = STANDARD
            .decode(
                pem.lines()
                    .filter(|line| !line.contains("KEY-----"))
                    .collect::<Vec<_>>()
                    .join(""),
            )
            .expect("PEM body is Base64");
        assert_eq!(&decoded[12..], &public[..]);
    }
}
