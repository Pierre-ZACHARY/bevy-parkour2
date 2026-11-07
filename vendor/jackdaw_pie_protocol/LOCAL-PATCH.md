# Local protocol patch

Source: jackdaw_pie_protocol 0.20.1-rc.1, crates.io (upstream 5a4492d61900f92eb4f867f3f90abf1a1f718831).
Registry files are never modified. The wire format is unchanged, so the
installed editor talks to the patched game as before.

The crate is excluded from the game's workspace so its protocol unit tests can
also run directly with its own manifest and lockfile.
Keyboard/mouse features are explicit rather than inherited from the game's
feature union. The snapshot test uses an absent synthetic component, so it
does not require camera features just to test archetype filtering.

`build_snapshot` reflects only each entity's own components (its archetype's),
instead of trying every registered type on every entity. The values streamed are
the same; measured on the game's scene (314 entities, 1763 registered types,
3458 components) one snapshot went from ~32 ms of registry walking plus ~8 ms
of serialization to ~4 ms. The runtime's PIE `stream_state` snapshots the scene
for the editor; see `vendor/jackdaw_runtime/LOCAL-PATCH.md` for its 10 Hz pacing.

`SKIP_PATHS` names `GlobalTransform` where it lives in Bevy 0.20
(`bevy_transform::components::global_transform`); the upstream entry named an
older module, so every entity's `GlobalTransform` streamed, although transform
propagation in the editor recomputes it from the streamed `Transform`.
