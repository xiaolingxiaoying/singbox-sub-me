# Prototype Instructions

Run the local server yourself and open the preview in the browser available to this environment. Do not give the user server-start instructions when you can run it.

Before making substantial visual changes, use the Product Design plugin's `get-context` skill when the visual source is unclear or no longer matches the current goal. When the user gives durable prototype-specific design feedback, preferences, or decisions, record them in `AGENTS.md`.

## Durable design direction

- The user chose the third generated concept, “Progressive Workspace,” as the source of truth.
- Aim for the calm simplicity and scanability associated with Clash Party, but do not copy its tile matrix, purple palette, colored skins, branding, or component shapes.
- Keep important proxy information visible: core state/version, outbound mode, system proxy, TUN, current node/latency/protocol, live rates, active connections, total traffic, subscription usage/expiry, traffic history, and recent events.
- Use progressive disclosure for lower-frequency DNS, route, experimental, node-list, and subscription-management details.
- Every production-facing visual must be reproducible in GPUI using nested flex layouts, solid colors, borders, radii, modest shadows, SVG/icon assets, canvas paths, scroll regions, and ordinary state changes. Do not rely on blur, glass, gradients, masks, CSS Grid, or animation-dependent meaning.
- Keep the core lifecycle button immediately to the right of the “连接状态” heading. Stopping the core is a guarded action and must show an explicit confirmation dialog before changing state.
- Use `crates/sbgui/assets/serein.ico` as the title-bar app icon. The title-bar brand lockup is the icon plus “Serein” only; do not show the redundant “sing-box 客户端” descriptor or version number.
- All prototype navigation destinations are in scope and must retain the same calm, light desktop-workspace visual system: narrow sidebar, quiet title bar, cool-gray canvas, white surfaces, fine borders, modest radii, and teal state accents. Extend only `prototypes/sbgui-progressive-workspace`; do not modify production `crates/` code for prototype work.
- In the Nodes destination, prioritize strategy groups over raw node inventory. Keep node rows intentionally compact: name, current selection, and latency only; hide protocol, region, host, and traffic until a later detail interaction is explicitly requested.
- The prototype title bar has no settings shortcut. Its only app-level utility control is the Chinese/English language switch; retain window controls alongside it. Keep reusable icon, color, layout, and acceptance references in `design-kit/`.
- Decided 2026-09-22: the production client keeps an 860 × 640 minimum window and narrow-screen support is dropped. The 820/560 breakpoints stay in the prototype as a density study but are not a production acceptance gate, and acceptance check 4 (390 × 844) is marked not applicable.
- Decided 2026-09-22: the Overview status card and metric strip are the sanctioned card surfaces — the prototype mock wins over the kit wording there. Every other destination still uses tables, lists, and two-pane rails; do not add card grids elsewhere.

When implementing from a selected generated mock, treat that image as the source of truth for layout, component anatomy, density, spacing, color, typography, visible content, and hierarchy.

Build app UI in `src/`. Keep `.openai/hosting.json`, `worker/index.js`, `scripts/prepare-sites-build.mjs`, and `tests/sites-worker.test.mjs` intact so the same local prototype can be handed to Sites. Before a Sites handoff, run `npm run build` and `npm run test:sites`; the build must leave `dist/client/index.html`, `dist/server/index.js`, and `dist/.openai/hosting.json`.
