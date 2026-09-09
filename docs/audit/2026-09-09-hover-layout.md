# Stable header links

The View library / View all hover shift was caused by egui 0.36's hover-only
button frame. Inactive frames omit the border; the hovered frame includes its
one-pixel stroke in layout, making the header two pixels taller and moving all
following content down. This was geometry, not animation or changing text.

Home links, discovery Refresh, and matching sidebar controls now keep their
hover background but explicitly use a zero-width stroke. Their footprint is
constant between inactive, hovered, and pressed states.

`cargo test --offline --lib section_links_do_not_move -- --nocapture` reproduced
the original movement from y=42 to y=44, then passed after the fix. The test uses
the real section helper and application style, alternates pointer entry/exit,
and checks both View library and View all. All 54 UI tests and strict all-target
Clippy passed. Formatting and whitespace checks passed.

The rebuilt native app also showed an unchanged playlist-card region when View
library was hovered (pixel comparison against the pre-hover capture). The app
was left open on Home with the existing queue and paused position preserved.
