# Layout Standard

## Desktop

- Title bar: 64 px high; Serein mark and wordmark on the left, language switch and window controls on the right.
- Navigation rail: 188 px wide; selected row uses teal text, pale teal fill, and a 3 px inset left indicator.
- Workspace: maximum 1440 px wide, 20 px horizontal padding, 18 px top padding.
- Working surface: white, 1 px `--serein-border`, 13 px radius, only a very light static shadow.
- Section rhythm: 12 px between surfaces; 18–26 px internal padding.
- Data views: use tables, lists, or two-pane rails rather than card grids.
- Node view: policy groups are the primary left rail; node choices are compact rows with name and latency only.

## Narrow screens

- At 820 px and below, sidebar becomes a 72 px icon rail; panels stack.
- At 560 px and below, navigation becomes a horizontal icon rail; title bar is 56 px.
- Toolbars stack vertically; large tables retain their own horizontal scroll region rather than causing page-level overflow.
- Controls must remain at least 34 px high and preserve focus visibility.

## Visual restrictions

- Use solid color surfaces only: no gradients, glass blur, masks, or decorative background art.
- Use teal for selected, enabled, and primary actions; green for healthy/success; blue only for upload-series data.
- Keep radii between 7 px and 13 px. Avoid large floating cards or bento grids.
