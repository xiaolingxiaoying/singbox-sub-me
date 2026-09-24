# Client config overrides merge with the server's semantics, in one place

The terminal client has a required feature, 覆写配置文件内容, that the server already
implements for administrators: deep-merge a JSON document onto a generated
configuration. The two must mean the same thing, because a user's override is
supposed to behave like the admin overrides they already know from ADR-0021 —
objects recurse, scalars and arrays replace, and an array under a key named
`rules` is *prepended* so a rule the user added wins over a generated verdict
without restating the whole list.

So the merge lives in `crates/json-merge`, one implementation, and the server
re-exports it from `src/override_template.rs`. A second copy inside
`crates/client-core` was the obvious shortcut and is what this ADR forbids: the
semantics are documented as verbatim under ADR-0021, and a client that
silently diverges would produce a configuration the administrator cannot
reproduce from the same document.

## The boundary

- Storage is one file per profile, `<data dir>/overrides/<sha256(profile)>.json`,
  written atomically and deleted with that profile's cache.
- Base is the cached subscription. The override merges onto it. Then the client's
  own control-channel fields are **written back**: the clash_api controller
  address and secret, the inbound list and mixed port the traffic mode chose, and
  `route/auto_detect_interface`. An override that names one of those is reported
  as a conflict, not silently honoured and not silently dropped — the rest of the
  document still applies, so the user can see which single field they cannot
  move.
- The merged text is handed to the existing `sing-box check` gate before the core
  starts. There is no separate validity model for overrides: if the result is not
  a configuration the pinned kernel accepts, the start fails loudly.
- A malformed or unreadable file on disk **blocks the start** rather than being
  skipped. "My override stopped applying" is far worse to debug than a refused
  start with the offending JSON pointer in the message.
- Visibility is read-only and redacted (`docs/client-description.md`): the client
  shows the merged outline, which fragments exist, which are enabled, and how
  many rules each contributes. The clash_api secret and any subscription
  credential in a URL are replaced before the text reaches a UI, per ADR-0013's
  discipline about diagnostics.

## Not chosen

Arbitrary JSON editing in the UI. The decision on record is fragment toggles plus
a read-only view; a hand-written document is supported (a bare object is one
implicit fragment) but is edited outside the client, the same way the server's
administrator templates are edited outside the daemon.

Letting an override own `inbounds` was also considered, and rejected: the traffic
mode already owns that list, because switching between system-proxy and TUN
rewrites it. Two owners of one list is how a profile silently loses its TUN
settings, which `crates/client-core` had to be fixed for once already.

## Consequences

`crates/sbtui` and `crates/sbgui` render the same `OverrideSummary`, so a toggle
made in either is visible in the other and survives a restart — enabled/disabled
state lives in the file, not in memory. Adding a rule-set style merge rule (say,
merge-by-name for `outbounds`) is now a single-crate change that both sides get,
and it must come with an ADR-0021-style amendment because it changes what a
frozen administrator document means.
