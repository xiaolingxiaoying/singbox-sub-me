# Direct subscription delivery with an IP fallback

sbctl defaults to directly serving the subscription endpoint on public ports 80 and 443, terminating TLS itself and using a mature external ACME client only for certificate lifecycle. This minimizes permanent processes and dependencies; when those ports already belong to another web service, sbctl instead supports a loopback external-reverse-proxy mode. Without a usable domain it exposes an explicitly lower-security HTTP IP fallback on a configured high port, rather than owning 80/443 or making short-lived IP certificates the default.
