# Fix release, runtime, and correction boundary details

sbctl uses a versioned manifest with a canonical JSON payload signed by Ed25519; the signature is standard Base64 and is verified before URLs or hashes are trusted. Direct HTTPS is implemented with two systemd `ListenStream` sockets passed through `LISTEN_FDS`, while Hyper/Axum supplies bounded HTTP handling. Certificate loading requires validity, SAN, private-key match, and SNI checks. Installation writes the ownership marker only after all files, units, daemon reload, startup, and health checks succeed.

Accounting state is written only by the reset timer or explicit administrator commands. Direction-aware corrections set RX/TX; total-only corrections remain a separate total adjustment. Ambiguous or nonexistent local reset times are rejected, and the reset task runs every minute with `Persistent=true` while cycle keys prevent unnecessary writes.
