# Direct HTTPS uses systemd socket activation

Direct subscription mode keeps public TCP 80/443, but systemd owns and opens those sockets and passes them to the non-root sbctl service. This avoids running sbctl as root or granting it broader capabilities, while preserving the product requirement that sbctl terminate HTTPS and serve ACME challenges itself.
