# Separate proxy credentials from subscription access

sbctl generates an independent proxy credential for every enabled protocol and a separate 256-bit subscription credential. It models the subscription host, optional proxy host, and VLESS Reality decoy SNI as different fields; proxy and subscription hosts may default to the same public hostname, while a Reality decoy SNI never does. This rejects the convenient shared-UUID pattern used by the upstream scripts in favor of narrower compromise scope and unambiguous configuration.
