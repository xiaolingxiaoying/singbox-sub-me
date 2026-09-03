# sing-box runs under a dedicated non-root account

The sing-box data plane uses a dedicated non-root service account because all Managed protocol listener ports are high ports and do not require root. Its unit receives only the filesystem and network access required by the generated deployment, reducing the impact of a data-plane compromise.
