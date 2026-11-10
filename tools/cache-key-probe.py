#!/usr/bin/env python3
"""Probe sccache Rust relocation with tiny libraries and an isolated LOCAL cache.
Run inside nix develop (or with rustc/sccache on PATH). Never writes to OVH.
"""
import json
import os
from pathlib import Path
import re
import shutil
import socket
import subprocess
import tempfile


def main():
    root = Path(__file__).resolve().parent.parent
    compiler = os.environ.get('PARKOUR_NIX_RUSTC') or shutil.which('rustc')
    sccache = root / '.compiler-cache/real' / ('sccache.exe' if os.name == 'nt' else 'sccache')
    with tempfile.TemporaryDirectory(prefix='rust-cache-key-') as temporary:
        work = Path(temporary)
        for name in ['a', 'b']:
            (work / name).mkdir()
            (work / name / 'lib.rs').write_text('pub fn answer() -> u32 { 42 }\n')
        with socket.socket() as listener:
            listener.bind(('127.0.0.1', 0))
            port = listener.getsockname()[1]
        # No AWS/remote configuration is inherited, including user config files.
        env = {k: v for k, v in os.environ.items()
               if not k.startswith(('SCCACHE_', 'AWS_', 'CARGO_'))}
        config = work / 'config.toml'
        config.write_text('')
        env.update(SCCACHE_CONF=str(config), SCCACHE_DIR=str(work / 'objects'),
                   SCCACHE_SERVER_PORT=str(port), SCCACHE_LOG='sccache::compiler::compiler=debug',
                   SCCACHE_ERROR_LOG=str(work / 'raw.log'),
                   SCCACHE_BASEDIRS=os.pathsep.join([str(work / 'a'), str(work / 'b')]),
                   CARGO_INCREMENTAL='0', PROJECT_CACHE_ACCESS='read-only')
        results = []
        try:
            for label, cwd, manifest in [('baseline', 'a', 'a'), ('repeat', 'a', 'a'),
                                         ('cwd-only', 'b', 'a'), ('cargo-env-only', 'a', 'b'), ('target-dir-only', 'a', 'a')]:
                call_env = dict(env, CARGO_MANIFEST_DIR=str(work / manifest))
                if label == 'target-dir-only':
                    call_env['CARGO_TARGET_DIR'] = str(work / '.jackdaw/target/game')
                subprocess.run([str(sccache), compiler, '--crate-name', 'relocation_probe',
                                '--crate-type', 'rlib', '--emit=link,dep-info', '--out-dir', '.', 'lib.rs'],
                               cwd=work / cwd, env=call_env, check=True)
                stats = subprocess.check_output([str(sccache), '--show-stats', '--stats-format=json'], env=env)
                results.append(dict(label=label, stats=json.loads(stats)))
        finally:
            subprocess.run([str(sccache), '--stop-server'], env=env, stdout=subprocess.DEVNULL)
        keys = re.findall(r'\[relocation_probe\]: Hash key: ([a-f0-9]+)', (work / 'raw.log').read_text())
        for result, key in zip(results, keys):
            result['key'] = key
        output = root / 'artifacts/cache-key-probe.json'
        output.parent.mkdir(exist_ok=True)
        output.write_text(json.dumps(results, indent=2) + '\n')
        print(output)
        for result in results:
            print(result['label'], result.get('key'), result['stats']['stats']['cache_hits'],
                  result['stats']['stats']['cache_misses'])
        if len(keys) != 5 or keys[0] != keys[1] or keys[0] in keys[2:]:
            raise SystemExit('Unexpected key behavior; inspect the evidence before drawing conclusions')


if __name__ == '__main__':
    main()
