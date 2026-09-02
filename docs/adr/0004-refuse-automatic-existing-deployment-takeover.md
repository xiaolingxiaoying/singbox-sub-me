# Refuse automatic takeover of existing sing-box deployments

When installation discovers an existing sing-box binary, service, or configuration, sbctl reports it and exits without changing it. A future explicit import workflow may migrate compatible deployments, but first-release installation only creates and owns a fresh sbctl deployment; this avoids destructive assumptions about `sing-box-yg` or manually maintained services.
