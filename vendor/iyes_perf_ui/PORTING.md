# Local Bevy 0.20.1 port

Source: https://github.com/IyesGames/iyes_perf_ui
Upstream version: 0.5.0
Upstream revision: 4873ba6fe2dfd3cd0e9f7bb20fa8df514b2fc292

The source tree and MIT/Apache-2.0 licenses are preserved. Example assets and
screenshots are omitted; this game uses Bevy's built-in font. The upstream README
describes the original release, whose Bevy version is 0.16.
The examples-only Bevy dev-dependency is omitted, allowing this path dependency's
unit tests to run from the game's locked dependency graph (`cargo test -p iyes_perf_ui --lib`).

Local changes target Bevy 0.20.1: root fonts use `FontSource` (the default resolves
Bevy's built-in font), text sizes use `FontSize`, and text justification uses
`Justify`; the bar widget uses per-edge `BorderColor::all`. Root setup applies the
configured global Z index. Widget value updates are suspended while the overlay
is hidden. The game enables only `entries` and `sysinfo`; its integration and custom
speed entry live in the binary crate, outside this vendor directory. The optional
upstream widgets, window entries and clock timezone feature are also compile-checked.

The game keeps this dependency optional through its `perf_ui` feature. Delivery
builds use `--no-default-features` and do not include it.

In this repository it is a workspace member (its tests run with the game's);
its manifest allows Clippy's `result_unit_err`, which upstream's API trips,
and turns off doctests: upstream's doc examples are fragments that do not compile
on their own.
