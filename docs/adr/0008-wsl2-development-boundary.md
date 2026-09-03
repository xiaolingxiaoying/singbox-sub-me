# WSL2 is a development host, not a production host

WSL2 Ubuntu 22.04 is supported for compilation, unit tests, and simulated CLI checks only; production lifecycle, networking, systemd boot behavior, certificate automation, and privileged port ownership must be verified on a real supported Debian/Ubuntu systemd host. This prevents a passing WSL2 test from being mistaken for evidence that a VPS installation is safe.
