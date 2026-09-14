# Client subscription profiles per sing-box version

The subscription serves one full-client sing-box artifact per supported minor
version (`sing-box-1.12.json`, `sing-box-1.13.json`, …) plus `sing-box-full.json`
targeting the newest stable release, each labeled with its supported range and
the version-specific field differences. The historical bare-`outbounds`
`sing-box.json` artifact keeps its exact bytes so existing clients never break.

Profiles are generated from one shared template with a small version-profile
registry (`SING_BOX_VERSION_PROFILES`) covering only the differences that the
upstream changelog research (`docs/research/sing-box-client-version-differences.md`)
confirms: the new DNS server object format (1.12+), rule actions replacing
block/dns outbounds and inbound sniff fields (1.13+), and legacy DNS removal
(1.14+). Fields deprecated but not yet removed (for example remote rule-set
`download_detour`) stay in every profile until their removal version, which
keeps one template valid from 1.12 through the latest stable.

Rule-set sources stay configurable (`client_rule_set_base_url`, default
jsDelivr + MetaCubeX/meta-rules-dat with `@sing` branches for sing-box `.srs`
and `@meta` branches for mihomo `.mrs`) so a mirror swap never touches the
generated templates, preserving the vendor neutrality of ADR-0018. A
`minimal` rule profile keeps every rule built-in for deployments that must not
contact a rule CDN at all.
