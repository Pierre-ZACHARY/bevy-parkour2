# WebGPU, Jackdaw runtime and binary assets

Update: the Basis prototype now builds with a pinned replacement loader and a
local runtime patch. See [BASISU-PROTOTYPE.md](BASISU-PROTOTYPE.md). The failure
below describes the original dependency chain, not the patched prototype.

Update 2026-11-05: the current game now runs through HTTP/WebGPU with a staged
BSB index, portable timing and browser-compatible shadows/textures. See
[WEBGPU-RUNTIME.md](WEBGPU-RUNTIME.md) for runtime proof and remaining boundaries.

Audit: 2026-11-04. Application baseline: a57a10d. Installed Jackdaw:
0.20.1-rc.1, source commit 5a4492d61900f92eb4f867f3f90abf1a1f718831.

## Reproduced failure

The real WebGPU build failed in basis-universal-sys 0.3.1 with
`fatal error: 'stdlib.h' file not found`, compiling C++ for
`wasm32-unknown-unknown`. Evidence:
`artifacts/1793747610118328500-57848/logs/000-webgpu-compile.log`.

Cargo's feature graph identifies this chain:

```
jackdaw_runtime/render
  -> bevy/basis-universal + compressed_image_saver_universal
  -> bevy_image
  -> basis-universal 0.3.1
  -> basis-universal-sys 0.3.1
```

Turning off the application's default features already removes editor IPC. It
does not remove this texture feature, which belongs to Jackdaw's render feature.
Removing render is not a valid replacement: the runtime's own documentation
states that a render-free configuration does not compile in this version.

## Similar cases and existing solutions

- [Bevy issue #17657](https://github.com/bevyengine/bevy/issues/17657)
  reports the same missing-header failure on Linux with wasm. It was still open,
  without comments, when checked. This is not specific to the Windows runner.
- [Tiny Takeoff / Rustunit](https://rustunit.com/blog/2024/12-16-rust-feature-debugging/)
  encountered the same error. They traced the enabling crate with Cargo's feature
  graph and removed an unnecessary Basis feature, enabling the required image
  functionality directly. That repair fits a dependency that does not actually
  need Basis; it cannot be copied unchanged into Jackdaw's texture importer.
- [Jackdaw PR #463](https://github.com/jbuehler23/jackdaw/pull/463), merged at
  2a7342b583b4e90dd69e5fb8362277897c8c4df3, introduced the texture import:
  Bevy's compressed saver produces Basis files and an asset reader can process
  textures into a filesystem cache at runtime.
- [basisu_c_sys](https://github.com/beicause/basisu_c_sys) supplies native and
  wasm bindings. Its current implementation uses vendored musl and Emscripten
  libc++/libc++abi and requires recent Clang. The related Bevy loader/saver are
  described as experimental, with glTF-loader integration limitations.
  [Bevy issue #23597](https://github.com/bevyengine/bevy/issues/23597), proposing
  upstream integration, remains open. This is an alternative implementation,
  not a drop-in Cargo feature for the pinned runtime.
- Targeted Jackdaw issue/PR searches for wasm and WebGPU found no direct fix.
  This search result is not proof that no unpublished solution exists.

## Why the application needs the runtime

`src/game.rs` registers JackdawPlugin and loads `scene.bsn` through
JackdawSceneRoot. The scene contains:

- a Ground Brush, whose geometry is rebuilt by Jackdaw;
- `materials/ground.material.bsn`, loaded through the Jackdaw catalog;
- a Player with the application's CharacterController component;
- a GltfSource referencing the Ninja;
- the light and camera.

The Avian bridge creates the collider from the authored ground mesh. The runtime
also supplies the BSN scene loader, reflected type registration and model/scene
spawning. Its role is different from the optional play-in-editor IPC.

Removing it is possible as an architectural change, but requires exporting
these objects into another runtime format or replacing their loading/spawning
code. Recreating the current scene manually would duplicate the authoring data.
A real offline conversion to Bevy-native runtime assets would preserve that
workflow, but needs a specified format and equivalence tests; it is a larger
change than the CI CLI.

See [Jackdaw's architecture documentation](https://jbuehler23.github.io/jackdaw/developer-guide/architecture.html).
The installed source is authoritative where online documentation differs.

## Initial correction analysis

A focused correction would separate texture importing/encoding from rendering.
The current checkout's tracked runtime assets contain no .basis, .ktx2 or .meta
files; its existing textures do not require runtime Basis compression.

However, removing the C++ dependency is only the first part:

1. Gate the texture-import module, registrations and wrapping reader behind a
   dedicated feature, enabled for the native authoring use that requires it.
2. Keep the rendering and scene-loading features on for WebGPU.
3. Replace filesystem-based catalog discovery on the browser with a packaged
   index and asynchronous AssetReader/AssetServer loading. In the pinned
   runtime, `load_asset_files` walks the filesystem and `asset_folder`
   falls back to `current_exe`; neither supplies the browser's material catalog.
4. Verify scene, material, glTF, movement graph and collider loading in an actual
   WebGPU browser, not just Cargo compilation.

The later prototype implements the feature split and typed Basisu loader settings
in a versioned local runtime copy. Registry source remains untouched. Browser
catalog discovery remains a separate limitation. See the prototype report for
what has actually been compiled and executed.

## BSN versus BSB

The packager exports tracked .bsn documents as .bsb in the staged package,
using the separate tools/asset-export Rust executable. Source assets remain
unchanged. Each encoding is decoded and compared to the original normalized
AST text and header before the staged text file is removed.

Jackdaw 0.20.1-rc.1 exposes `text_as_binary`, `convert_to_binary` and
`export_binary` in jackdaw_bsn. This is genuine encoding, not an extension rename.
The format retains Jackdaw's AST and headers. The runtime still uses Jackdaw's
decoder and scene-loading logic.

The binary-only package implementation:

- encodes documents only in the staged package, preserving repository .bsn files;
- registers JackdawAssetSourcePlugin before DefaultPlugins so references written
  as .bsn resolve to .bsb twins;
- prevents CharacterController's default-graph installer from recreating a .bsn
  when the shipped .bsb is present;
- requires runtime verification from an extracted package with no .bsn files
  and an unrelated working directory;
- invokes a small separate Rust asset-export helper using jackdaw_bsn, keeping
  the xtask CLI itself independent of Bevy.

The graph loader already decodes document bytes through
`jackdaw_bsn::document_text_from_bytes`, but registers the .flowgraph.bsn
extension; the twin-resolving reader therefore matters. Binary packaging alone
does not solve the browser filesystem or Basis compilation issues.
