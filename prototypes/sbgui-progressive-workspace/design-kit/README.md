# Serein Prototype Design Kit

This folder is the handoff package for the web prototype only. It centralizes the reusable visual rules, icon inventory, palette, layout behavior, and acceptance criteria used by `src/`.

## Contents

- `assets/serein.ico` — the approved application mark.
- `IconSet.jsx` — import-ready registry of every Phosphor icon used by the prototype.
- `icon-manifest.md` — icon meanings, sizes, and states.
- `colors.css` — named color and typography tokens.
- `layout.md` — desktop and narrow-screen layout rules.
- `acceptance.md` — visual, functional, accessibility, and build acceptance criteria.

## Usage

Use the tokens and registry when adding another prototype surface. Keep real UI controls, navigation, labels, and tables in React/HTML; use the icon registry rather than adding ad-hoc SVGs or emoji. This package describes the prototype and does not modify production GPUI code.

## Language behavior

The title-bar language button toggles Chinese and English for the current prototype. It switches the navigation, headers, controls, table labels, settings labels, placeholders, and action copy. Data values such as IPs, ports, versions, timestamps, protocols, and traffic units intentionally remain unchanged.
