# Subscription content templates are a compile-time catalog

Client artifacts were thin: one proxy group, no routing verdicts beyond what a
bare profile needs, no sniffing configuration, and rule sets that vanished
wholesale under `client_rule_profile = "minimal"`. Richer content is a product
decision per *client generation*, so it is expressed as a template axis:

```text
canonical nodes → template (built-in) → client_rule_profile (CDN or not) → overrides (two administrator files) → artifact
```

`ClientTemplate { Standard, Global, Split }` lives in `src/subscription/template.rs`
as a compiled-in `TemplateSpec` (groups, rule sets, inline rule twins, DNS,
sniffing, final group). It is not a directory of files on disk, and it does not
reuse the `ClientRuleProfile` axis.

**`Standard` reproduces today's output byte-for-byte.** The artifact goldens in
`src/subscription/snapshots/` are the proof: introducing the axis must move zero
bytes, and any golden that moves is a deliberate product decision recorded with
an ADR amendment, not an implementation detail (ADR-0021).

Why compile-time rather than administrator-editable templates:

- a single template file cannot be valid for five sing-box minor versions at
  once, and ADR-0021 makes a malformed override abort regeneration. Putting the
  *default* content behind that rule would let one bad paste take down the
  default subscription for every client;
- the actual request behind the gap is "the artifacts are too thin", not "the
  templates must be editable by an administrator".

Why a separate axis rather than folding richness into `client_rule_profile`:

- `minimal` today means *delete the whole external-resource class* — no CDN rule
  sets, and therefore no split routing either. That is the regression this
  phase exists to fix. `minimal` keeps exactly one meaning, "never contact a
  rule CDN", and every
  template rule set ships an inline rule twin rendered from a compiled-in
  domain/IP list, so routing survives without a CDN.

Layering order is fixed and load-bearing: the template decides *what content
exists*, `client_rule_profile` decides *whether external resources are reachable*,
and administrator overrides are applied last so an operator can add or prepend
without editing generated structure. `client_rule_set_base_url` remains the only
CDN knob.

Out of scope: administrator-supplied template files, per-request template
selection (the subscription API still rejects query parameters, ADR-0002), and
any change to the bare `sing-box.json` / `uri` / `uri.txt` / `shadowrocket.txt`
artifacts, whose bytes are frozen. Enabling `--client-template split` may grow
groups, rule sets and `sniffers` in the full profiles; those four artifacts must
stay byte-identical, which is asserted by the goldens.
