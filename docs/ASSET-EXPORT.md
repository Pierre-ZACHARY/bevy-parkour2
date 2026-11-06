# Asset quality presets

Asset quality is independent of the Cargo compilation profile. In the isolated
`ci-rust` worktree, build with:

```text
cargo xtask build --targets webgpu --asset-quality optimized
cargo xtask build --targets windows-x64 --asset-quality optimized
cargo xtask plan --targets windows-x64,webgpu --asset-quality optimized
```

`build`, `ci` and `plan` accept `--asset-quality` (default `original`) and
`--asset-presets path/to/presets.json`. The same named quality can resolve to
different recipes for each target. The versioned catalog is
[`tools/asset-export/presets.json`](../tools/asset-export/presets.json).
The manual GitHub workflow also exposes an `asset_quality` choice; automatic
builds keep `original` unless their CLI invocation selects another preset.

| Quality | Target | Recipe |
|---|---|---|
| `original` | All five supported desktop/Web targets | Preserve dimensions/channels; Web still requires 16-to-8-bit PNG compatibility conversion |
| `optimized` | WebGPU | `web-2k`: eleven 4096px textures reduced to 2048px; nine opaque RGBA textures become RGB |
| `optimized` | Windows x64, Linux x64/ARM64, macOS ARM64 | `desktop-lossless`: nine opaque RGBA textures become RGB, original dimensions/bit depth retained |

Android/iOS have no export recipe yet because their package builders are not
implemented. No Basis conversion of the Ninja glTF is introduced here: distributed
images remain PNG and keep their names, so glTF references stay valid.

## Editing a preset

Each recipe lists exact paths relative to the staged `assets` directory. For
example, the Web recipe includes:

```json
{
  "path": "character/ninja/textures/T_HEAD_F_BaseColor_14_02.png",
  "semantic": "color",
  "channels": "rgb",
  "max_size": 2048
}
```

`max_size` limits the longest side, preserves the aspect ratio and never
upscales. Omitting it preserves the dimensions. `channels` is `preserve` or
`rgb`; RGB conversion refuses any non-opaque alpha, rather than silently
destroying transparency. Hair and eye base-color alpha are preserved.

`semantic` controls Lanczos3 resampling: `color` uses linear-light sRGB and
premultiplied alpha; `normal` filters tangent-space vectors and renormalizes;
`data` filters linear independent channels, for packed ORM maps. The exact
Web list of sixteen textures, including unchanged smaller images, is in the
catalog. New unlisted textures retain dimensions/channels; the mandatory Web
16-bit compatibility conversion applies to every PNG. Extend the explicit list
when adding textures that should be optimized.

To add another quality, add a `presets.<name>` mapping from target names to
recipe names, then add the corresponding `recipes.<name>` array. Unknown
fields, targets without a preset, missing files, duplicate rules, traversal,
symlinks and unsafe alpha removal fail the export.

## Source protection and provenance

The packager copies tracked runtime assets into a fresh package directory,
then runs the Rust exporter there. Source PNGs, glTFs, BINs and BSNs are never
rewritten. The exporter rejects the repository source assets directory.
BSN documents still become verified BSB documents for every package.

The helper can also run on a disposable copy:

```text
cargo run --locked -p asset-export -- <staged-assets> --target webgpu --quality optimized
```

Use `--presets <catalog.json>` for a custom catalog. The package's
`assets/asset-export.json` inventories **every PNG**, changed or unchanged,
with effective rules, before/after dimensions, color types, bytes and SHA-256.
It records the quality, resolved recipe and catalog fingerprint. The package
manifest records these selections and hashes the report along with all runtime
files. A repeat export verifies hashes and does no further conversion; changing
the preset or staged images requires a fresh source copy. A failed operation
must be retried through a fresh package build, not promoted for distribution.

Quality changes can be reviewed in this catalog without touching authoring
assets. Desktop recipes are configured for all supported desktop targets;
actual native package execution still requires each matching runner.

## Observed validation (2026-11-05)

Web run: `artifacts/quality-presets/1793871434743741400-48008` (Cargo `ci`
profile, wasm-bindgen and wasm-opt, same WASM SHA-256 as the earlier original
package `1793868682175410900-35872`).

| Payload | Original bytes | Optimized bytes |
|---|---:|---:|
| Web PNGs | 134,464,172 | 33,488,370 |
| Complete Web ZIP | 158,406,051 | 58,816,807 |
| Desktop PNGs | 165,257,647 | 145,874,231 |

The Web ZIP is 62.87% smaller. Its full file inventory, ZIP members, CRC and
SHA-256 were verified; source hashes remain unchanged. The report inventories
sixteen PNGs, fourteen changed, eleven resized and nine RGBA-to-RGB conversions.
The desktop asset-only export preserved every decoded pixel and every image
dimension, verified independently with Pillow; this is not a native executable
or cross-platform package launch test.

In Paseo's integrated WebGPU browser, the optimized package fetched the new PNGs,
loaded its catalog and graph, and rendered one camera / 24 meshes. The initial
GPU screenshot `browser/optimized.png` was inspected alongside the original
capture: character, ground, shadow and hair silhouette are visible. This is not
a close-up assertion of texture quality equivalence. Logs show twelve hair mip
levels for the 2K assets versus thirteen for the original 4K assets, with no
render errors in the observed load/capture interval.

The later movement recheck is **INCOMPLETE**: after initial rendering, Paseo's
tab ceased delivering even an independent `requestAnimationFrame` callback
(zero over 600 ms); DOM timers still ran. A reload reached adapter initialization
but did not publish game frames. This observation does not establish a game
input regression or a sustained-runtime pass. See `browser/verification.json`.
The unchanged WASM and earlier movement validation remain separate evidence.

Exporter tests: seven passed; xtask: eighteen passed and one opt-in acceptance
test ignored. Rustfmt, Clippy with denied warnings, and workflow YAML parsing
passed. The per-file asset results are in `assets/asset-export.json`; independent
package/source checks are in `asset-quality-verification.json` alongside the
Web run and `artifacts/asset-quality-20261105/desktop-verification.json`.
