# Official sing-box downloads use GitHub asset digests when available

The direct official sing-box install and update path resolves the latest stable
release through the GitHub Releases API. It selects the exact Linux archive for
the requested version and host architecture, then compares the downloaded
archive's SHA-256 with the matching asset's `digest` field before extraction.

If the API omits the matching asset digest or returns it as `null`, the command
prints a warning and continues with the existing archive extraction and
`sing-box version` checks for compatibility with releases that do not expose a
digest. A malformed digest, missing expected asset, or digest mismatch fails
closed before any managed binary is replaced or service is restarted.

The GitHub API digest is an integrity check against the value GitHub reports; it
is not an independent publisher signature and does not establish build
provenance. Deployments that require publisher-authenticated artifacts must use
the fixed-version signed release manifest path (ADR-0010). The direct path's
trust boundary is HTTPS plus GitHub's release metadata when an asset digest is
available, followed by archive and runtime compatibility checks.
