# Benchmark Validation: Alacritty / WezTerm (NFR3)

Collected at plan time (2026-07-05) from official repositories, changelogs,
and issue trackers only. No code from either project was read or reused;
implementation is written independently (project benchmark reference policy).

## 1. Alacritty damage tracking

Facts
- Tracking unit is **per-line + an aggregated rectangle**: each line keeps a
  damage state bounded by its leftmost/rightmost changed cells; overlapping
  rects are merged before compositor reporting
  ([PR #2724](https://github.com/alacritty/alacritty/pull/2724),
  [PR #5773](https://github.com/alacritty/alacritty/pull/5773)).
- Cursor / selection / URL highlights are explicitly called out as
  out-of-band cell state: the **previous position must be retained and both
  old and new positions damaged**, or ghosting results (PR #2724 review).
- Recorded pitfalls: rect **off-by-one** ghosting at cell edges; **resize
  with stale highlight coordinates → panic** (fixed by clearing cached state
  on resize); damage omissions on UI elements.
- Damage reporting landed in v0.11.0
  ([changelog](https://alacritty.org/changelog_0_11_0.html)).

Implications for emterm
- Line-granularity damage (what Stage 1/2 build on) is the proven
  cost/benefit sweet spot; per-cell tracking unnecessary.
- Old+new cursor/selection rows MUST both be dirtied (task0002 design).
- Clear out-of-band cached state on resize (task0003 invalidation: resize →
  full cache drop).

## 2. WezTerm shaped line cache

Facts ([DeepWiki Rendering Pipeline](https://deepwiki.com/wezterm/wezterm/3.2-rendering-pipeline),
[changelog](https://wezterm.org/changelog.html))
- Multi-layer caching: shape cache (shaping results), line cache (line
  sequence number + shape hash decides re-shape), quad cache (final GPU
  vertex data keyed by shape hash + selection + cursor + config generation).
- Invalidation triggers: selection/highlight change, cursor move, config
  generation change (incl. fonts), text change (shape hash), resize.
- Recorded pitfall (20220807): **forgetting to invalidate the shape cache on
  reverse-video changes left stale rendering** — every appearance-affecting
  attribute must be an invalidation trigger.
- Glyph-fallback updates force a re-shape.

Implications for emterm
- Row cache keys/invalidation must cover every appearance-affecting input:
  content, selection, hover, search, theme/font (config), reverse video and
  similar SGR attributes are part of content → covered by content dirty rows.
- The equivalence test (TS-4) is the systematic guard against the "missed
  trigger" class of bugs both projects hit.
- emterm's task0003 caches at the instance level (post-style-resolution),
  one layer rather than WezTerm's three — acceptable because the equivalence
  postcondition bounds the blast radius; selection/cursor-only changes
  rebuild affected rows instead of re-shaping the whole grid.

## 3. Idle event-loop policy

Facts
- **Alacritty uses true event-driven `ControlFlow::Wait`** — blocks with no
  events; redraw happens in response to redraw requests
  ([winit #1619](https://github.com/rust-windowing/winit/issues/1619)).
  PTY readable is awaited on the PTY event-loop thread (level-triggered
  polling) rather than timer-polled from the UI loop.
- Cursor blink is a **scheduled timer** toggling a visibility flag, not
  per-frame polling. Alacritty additionally stops blinking after an
  inactivity timeout (default 5 s, v0.11.0).
- WezTerm is likewise event-driven, redrawing only on change (idle details
  thinner in official docs).

Implications for emterm
- FR5's design (true `Wait`, PTY reader wakes the loop via the existing
  proxy channel, blink on a `WaitUntil` deadline only while enabled+focused)
  matches Alacritty's proven approach.
- Blink inactivity timeout is a possible future refinement — noted only, out
  of scope for this feature (not in SPEC).

## Sources

- https://github.com/alacritty/alacritty/pull/2724
- https://github.com/alacritty/alacritty/pull/5773
- https://alacritty.org/changelog_0_11_0.html
- https://wezterm.org/changelog.html
- https://deepwiki.com/wezterm/wezterm/3.2-rendering-pipeline
- https://github.com/rust-windowing/winit/issues/1619
