#!/usr/bin/env sh
# Thin Unix bootstrap; installation and cache policy are implemented in Rust.
set -eu
repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cache_dir="$repo_root/.compiler-cache"
mkdir -p "$cache_dir"
if [ ! -x "$cache_dir/setup" ] || [ "$repo_root/tools/compiler-cache.rs" -nt "$cache_dir/setup" ] || [ "$repo_root/tools/compiler-cache-pins.txt" -nt "$cache_dir/setup" ] || [ "$repo_root/tools/cache-config.rs" -nt "$cache_dir/setup" ] || [ "$repo_root/tools/cache-infisical.rs" -nt "$cache_dir/setup" ] || [ "$repo_root/tools/infisical-config.json" -nt "$cache_dir/setup" ]; then
    rustc --edition=2024 -C opt-level=2 "$repo_root/tools/compiler-cache.rs" -o "$cache_dir/setup"
fi
"$cache_dir/setup" --setup
