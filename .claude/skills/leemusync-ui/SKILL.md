---
name: leemusync-ui
description: Use when creating or changing any LeemuSync Flutter screen, widget, theme value, animation, navigation route or user-facing text, or when reviewing the UI for platform parity, accessibility or jank.
---

# LeemuSync UI

Three unbreakable rules: F1 same look everywhere, F2 no jank and nothing broken, F3 same navigation everywhere.

## Read first (sections only)
`docs/specs/ui/design-system.md` (tokens, components, Performance) · `docs/specs/ui/navigation.md` (destinations, plus the flow you're touching) · `docs/specs/i18n.md` (Terminology).

## Recipe
1. Reuse a component from `app/lib/design/`. A new component goes into the design-system component table.
2. Take every value from tokens (`app/lib/design/tokens/`): no literal colors, sizes, durations or font families.
3. Don't branch visuals by platform. `Platform.isX` is only for the behaviours F1 lists as "may differ".
4. Lay out by size class (compact < 600 dp, medium < 1200 dp, expanded), with the same steps and order in every class.
5. Strings go through `AppLocalizations`. Add each to `app_en.arb` (with `@description`) and `app_es.arb` in the same change.
6. Design loading, empty, error and offline states. Add semantics labels, 48 dp touch targets and keyboard focus on desktop. Respect reduce-motion and 200% text scale.
7. Performance: `const` widgets, lazy lists, `RepaintBoundary` around animated status, no opacity or blur layers in scrolling content, status animations driven by engine events (no polling).
8. Tests: widget test + goldens for compact/medium/expanded × light/dark × en/es. An integration test if a flow changed. A profile-mode frame check for animations.
9. A new route or flow → update `navigation.md` in the same commit.

## Common mistakes
| Mistake | Fix |
|---|---|
| Default Material/Cupertino look | Custom theme built from tokens |
| System fonts | Bundled fonts only |
| A feature reachable only on desktop | Same flow in every size class |
| Spanish text clipped | Goldens in `es`; allow wrapping and flex |
| Accepting golden diffs blindly | Review every diff image |
