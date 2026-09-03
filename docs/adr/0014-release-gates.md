# Production release gates are mandatory

Passing Rust tests and Clippy is insufficient for release. A release must also pass a real Debian/Ubuntu systemd acceptance environment, prove non-root startup and port ownership, verify an authenticated fixed-version manifest, and demonstrate installation/update rollback under injected failure.
