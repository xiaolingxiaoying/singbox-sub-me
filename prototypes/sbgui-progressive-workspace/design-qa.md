# Design QA — Serein Progressive Workspace

- Source visual truth: `design-reference.png`
- Implementation screenshot: `implementation-1440x1024.png`
- Combined comparison: `design-comparison.png`
- State: overview, connected, system proxy enabled, TUN disabled, rule mode, all disclosure rows collapsed
- Browser viewport: 1440 × 1024 CSS px
- Device pixel ratio: 1.0
- Source pixels: 1487 × 1058
- Implementation capture pixels: 1425 × 1013 (browser content area after scrollbars/chrome)
- Normalization: both images scaled proportionally to 1024 px height and combined horizontally for the final visual comparison

## Full-view comparison evidence

The final side-by-side comparison preserves the selected concept's composition: narrow left navigation, three broad horizontal information groups, connection controls in the first group, node/metrics/chart in the second, and subscription/events in the third. The same cool-neutral surface system and single teal accent remain intact. The production prototype is slightly denser than the generated concept, intentionally retaining more event and control detail without introducing additional cards.

## Focused-region comparison evidence

The full-view comparison is sufficient because the target contains no raster illustration, photography, complex texture, or asset crop. Focused inspection was performed on the top status/control row, node and metric strip, chart axes/legend, and subscription/event split. Icons use Phosphor rather than handcrafted SVGs; the traffic plot uses canvas paths, matching the GPUI implementation constraint.

## Findings

- [P3] The implementation keeps a small core lifecycle action immediately beside the “连接状态” heading; it is not visible in the generated concept.
  - Location: connection status heading row.
  - Evidence: the source shows status plus proxy/TUN/mode; the implementation also exposes core lifecycle control.
  - Impact: minor extra visual weight, but it preserves an existing high-value Serein action and keeps it associated with the status it changes.
  - Disposition: accepted product constraint. Stopping is guarded by an explicit confirmation dialog.

- [P3] The implementation uses “香港 03” instead of the generated image's mixed-language “Hong Kong 03.”
  - Location: current node summary.
  - Evidence: visible in the combined comparison.
  - Impact: none; Chinese naming is consistent with the existing product and supplied GUI data.
  - Disposition: accepted localization improvement.

## Required fidelity surfaces

- Fonts and typography: Segoe UI Variable/Text stack, weights and hierarchy match the native desktop direction. The first pass was too compressed; heading, navigation, metric, status, and event text were enlarged before the final capture.
- Spacing and layout rhythm: sidebar and major-region proportions match. The first pass left excessive blank space; minimum section heights and chart height were increased. Final vertical overflow is 49 px, keeping the complete third section reachable without hiding persistent controls.
- Colors and visual tokens: cool gray canvas, white working surfaces, teal primary state, green success, blue upload, subtle gray dividers. No purple, gradients, blur, or glass effects.
- Image quality and asset fidelity: no raster assets are required by the target. Icons come from Phosphor; the chart is drawn at device pixel ratio with canvas for crisp rendering.
- Copy and content: core/version, mode, proxy port, TUN state, node/protocol/latency, live rates, connections, traffic total, subscription usage/expiry, chart, and recent events are all present.

## Interaction and responsive verification

- System proxy switch changed from enabled to disabled and updated `aria-checked`.
- Global outbound mode became selected.
- Advanced settings expanded and revealed DNS, route, and auto-restart detail.
- Core stop action opened a confirmation dialog without changing state; cancel preserved the running state; “确认停止” changed the status to “已停止”; direct restart restored the connected state.
- 390 × 844 viewport: no horizontal overflow; navigation becomes an icon rail and the main sections stack.
- Browser console: no warnings or errors in the final desktop pass.

## Comparison history

1. Initial capture: P2 information density and typography were too compressed relative to the selected concept; the layout ended early and lost the intended calm reading rhythm.
2. Fix: increased all major section heights, chart height, navigation/body/status typography, metric values, event row height, and bottom-section breathing room.
3. Post-fix evidence: `design-comparison.png` shows matching three-section hierarchy and substantially closer vertical rhythm. No actionable P0/P1/P2 differences remain.

## Navigation expansion QA

- Extension source visual: `design-other-pages.png`, a coordinated desktop specification for Nodes, Subscriptions, Rules, Connections, Logs, Settings, and About.
- Visual consistency checks: shared 188 px sidebar, quiet 64 px titlebar, cool-gray canvas, white surface panels, thin #dce4e1 borders, 7–13 px radii, teal selected states, and the original system type scale are applied across all eight destinations.
- Screen coverage: Nodes has searchable/selectable rows; Subscriptions shows usage and update actions; Rules has mode selection, search, and individual enabled states; Connections and Logs use readable data tables; Settings exposes grouped, working toggle rows; About includes version and update state.
- Interaction checks: node search returned one Japanese row; rule search returned one GitHub rule; a rule toggle changed its `aria-checked` state; log pause became resume; DNS settings opened with a readable cache toggle; all primary navigation destinations rendered.
- Responsive check: 390 × 844 viewport preserved a usable icon navigation rail, stacked toolbar controls, and placed wide data tables in their own horizontal scroll container without viewport-level horizontal overflow.
- Browser console: no warnings or errors after the navigation expansion.
- Intentional deviation: the design-board reference is a visual-system and information-density specification, not a literal source of user-facing copy. The prototype preserves the existing Serein labels and data where available.

final result: passed
