# Design system

> **Status:** Draft · **Related:** [rules](../../rules.md) (F1, F2, P5, P6), [navigation](navigation.md), [status-and-notifications](status-and-notifications.md), [i18n](../i18n.md), [testing](../testing.md), [skill: leemusync-ui](../../../.claude/skills/leemusync-ui/SKILL.md) · **Code:** `app/lib/design/` (planned)

## Purpose
One visual language that looks identical on every platform (F1), feels instant (F2), and is simple for players while keeping power tools one level deeper (P5).

## Principles
1. **Status first.** The first thing every screen answers: "are my saves safe and in sync?"
2. **Calm, playful, never childish.** Game-flavoured warmth, clean structure.
3. **Progressive disclosure.** Defaults on the surface, power features under "Advanced". Nothing is CLI-only on desktop.
4. **Honest states.** Every async element has loading, empty, error and offline designs. No silent failures.

## Parity
- Built on Flutter's Material widget infrastructure (text input, scrolling, selection, accessibility) with a **fully custom theme**: no stock Material look and no Cupertino widgets.
- **Bundled fonts** (OFL-licensed UI sans + monospace for paths and ids), never system fonts, so metrics match everywhere. Chosen in the Phase 3 design exploration.
- One icon set, bundled (permissive license), used across app, tray and notifications.
- Allowed per-platform differences are listed in [rules F1](../../rules.md#f1-same-look-on-every-platform).

## Tokens
All values come from `app/lib/design/tokens/`. Widgets never hard-code colors, sizes or durations.

| Group | Contents |
|---|---|
| Color | Semantic roles: `surface`, `onSurface`, `accent`, `synced`, `syncing`, `attention` (lock/conflict), `danger`, `offline`. Light, dark and high-contrast sets. WCAG 2.2 AA contrast minimum |
| Type | One scale (display → caption) built from the bundled fonts. Respects OS text scaling up to 200% |
| Space | 4-pt scale |
| Shape | Radius scale |
| Motion | Durations 100/200/300 ms, standard easing curves. Respects OS "reduce motion" |
| Elevation | A few levels, expressed through tone, not heavy shadows |

## Brand
- **Name:** LeemuSync (lemur + emu + sync).
- **Direction:** a ring-tailed lemur whose striped tail curls into a sync loop. The rings double as the sync/progress motif in the app icon, tray glyph and progress ring. Final art comes from the Phase 3 design exploration.

## Components
The inventory grows with the code. Each component gets a widget test and goldens.

| Component | Purpose |
|---|---|
| `AppShell` | Adaptive container: bottom bar / rail / sidebar ([navigation](navigation.md)) |
| `StatusHero` | Large current status (synced, syncing n changes, attention, offline) |
| `GameCard` | Game, emulator, tier badge, last synced, lease state, Play button |
| `PlayerAvatar`, `DeviceChip` | Who and where |
| `LeaseBadge` | "In use on Desktop · 2 min" |
| `ConflictCompare` | Side-by-side heads with metadata and choose action |
| `VersionTimeline` | History with restore |
| `PathField` | Power-user path input: shows the resolved path, grant state and validation |
| `BackendCard` | Storage provider, health, capabilities |
| `ProgressRing` | Lemur-ring progress, determinate and indeterminate |
| `Banner`, `Toast`, `EmptyState`, `ErrorState` | Feedback |
| `WizardStep` | Setup and join flows |

## Accessibility
Screen-reader semantics on every interactive element. Full keyboard navigation and visible focus on desktop. Touch targets ≥ 48 dp. Text scaling to 200% without clipping. Color is never the only signal (status also uses icon and text).

## Performance
Applies everywhere. Checked in profile mode ([testing](../testing.md)).

| Budget | Target |
|---|---|
| Frame rate | 60 fps everywhere. 120 fps on high-refresh displays |
| UI + raster time per frame | ≤ 8 ms on reference devices (headroom for 120 Hz) |
| Cold start to first frame | ≤ 1 s desktop, ≤ 1.5 s mobile |
| Jank | No frame over budget during navigation, scrolling or status animation in perf tests |

Techniques: `const` widgets; lazy lists; no `saveLayer`-heavy effects (opacity layers, blurs) in scrolling content; `RepaintBoundary` around animated status; status animation driven by the engine's event stream, never polling; images pre-sized and cached.
