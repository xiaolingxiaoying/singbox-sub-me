# Server-side override templates for client artifacts

Client-facing artifacts (the full sing-box profiles and the clash/mihomo
artifacts) merge administrator-maintained override templates from
`etc/sbctl/overrides/` (`sing-box-override.json`, `clash-override.yaml`) at
generation time, instead of asking every client app to maintain its own
override file. This moves the previous Clash Party-side manual override (for
example the ChatGPT/OpenAI and X.com routing rules) into the controlled,
atomic, and rollback-safe artifact transaction.

Merge semantics are deliberately simple and documented verbatim in
`sbctl config override show`:

- objects merge recursively; every other type replaces wholesale;
- arrays replace wholesale, except an array under a key literally named
  `rules`, which is **prepended** to the generated array so an override rule
  wins against the generated verdicts without restating the whole list;
- the historical bare-`outbounds` `sing-box.json` and the URI artifacts are
  never overridden, preserving their byte compatibility.

The subscription HTTP API keeps rejecting query parameters (ADR-0002): the
override is a server-side concept, not a per-request rewrite. A malformed
override file aborts regeneration, so the transactional writer keeps serving
the previous known-good artifacts instead of a broken merge.
