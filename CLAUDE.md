# CLAUDE.md

eMterm is a native terminal emulator for Linux and Windows with a wgpu+swash
render pipeline and child WebView windows for rich Markdown / JSON / YAML /
image display and the settings panel.

## Who it is for

Developers who use Claude Code on Linux and Windows.

## Product value

A modern terminal emulator that combines traditional terminal reliability with
rich content rendering. It displays images and formatted Markdown / JSON / YAML
directly in the terminal via control sequences while keeping latency low.

- Full ANSI control sequence support
- Kitty Graphics Protocol / SIXEL for inline images
- Custom OSC extension for Markdown / JSON / YAML rendering in child WebView
  windows
- mux: tmux-style multiplexing (windows / tabs / panes) inside one process
- Low-latency typing with a wgpu render pipeline driven by the winit event loop
- Stateless CLI helpers that work over SSH

## Design philosophy

- AI-first: prioritizing compatibility with Claude Code
- Explicit display commands only (no auto-detection)
- Stateless CLI design (works over SSH)
- Robust isolation (XSS protection in child WebViews, resource management)
- Material Design 3 baseline
