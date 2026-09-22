# Acceptance Standard

## Required checks

1. `npm run build` completes successfully.
2. `npm run test:sites` completes with all tests passing.
3. Browser verification at desktop width confirms every destination: Overview, Nodes, Subscriptions, Rules, Connections, Logs, Settings, and About.
4. Browser verification at 390 × 844 confirms no viewport-level horizontal overflow; wide tables may scroll inside their own container.
5. The language button changes the visible interface to English and back to Chinese without losing the active page.
6. The title bar contains no settings shortcut; only the language control and window controls remain on the right.
7. Node screen exposes policy groups and compact node choices; compact rows show only selection, name, and latency.
8. Core controls remain guarded: stopping opens confirmation before state changes.
9. Browser console contains no `error` or `warn` entries caused by the prototype.

## Visual sign-off checklist

- Cool-gray canvas, white surfaces, thin borders, and teal active states match `colors.css`.
- Sidebar, titlebar, control heights, spacing, radii, typography, and icon weights follow `layout.md` and `icon-manifest.md`.
- All icon references come from `IconSet.jsx` / Phosphor; no improvised emoji or text-symbol icons are introduced.
- English copy is readable, labels remain unwrapped where space allows, and data values remain stable.

The feature is accepted only when every required check passes and no visible layout regression appears in the current browser preview.
