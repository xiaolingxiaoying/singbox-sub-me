# Direct HTTPS uses a non-root service with narrow port privilege

Direct subscription mode remains responsible for public TCP 80/443, but the long-running sbctl service must not run as root. The implementation will use systemd socket activation or the narrowly scoped `CAP_NET_BIND_SERVICE`, with systemd hardening and a separate service account, so public port ownership does not expand the daemon's filesystem or operating-system authority.
