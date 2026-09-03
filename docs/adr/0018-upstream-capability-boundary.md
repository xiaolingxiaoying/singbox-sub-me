# Keep upstream compatibility behavioral and provider-neutral

sbctl adopts the reusable capabilities found across the `vps-sub-meter` scripts: interactive configuration, interface detection, configurable accounting, three subscription formats, atomic artifact replacement, certificate checks, and systemd lifecycle management. It does not adopt fixed-expiration subscriptions, Basic Auth, upstream URL aliases, provider-specific entrypoints, WARP/IPv4/IPv6 switching, Caddy ownership, Argo tunnels, or automatic firewall changes in the first release.

Compatibility means matching the generated protocol fields and client-consumable subscription content, not importing upstream paths, credentials, services, file layouts, or provider-specific behavior. This keeps the single-administrator product boundary small and prevents unrelated host networking features from entering the control plane.
