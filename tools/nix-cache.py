#!/usr/bin/env python3
"""Copy signed Crane outputs to/from the private OVH binary cache.

Run inside `nix develop`: python3 tools/nix-cache.py pull|push [.#game .#game-deps].
Downloads run in the client so multi-user Nix daemons need no AWS secret file.
"""

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
from urllib.parse import urlencode

import boto3
from botocore.config import Config
from botocore.exceptions import BotoCoreError, ClientError


ROOT = Path(__file__).resolve().parents[1]


def run(command, **kwargs):
    return subprocess.run(command, check=True, text=True, **kwargs)


def secret(name, project, path, metadata):
    result = subprocess.run(
        ["infisical", "sm", "secrets", "get", name, "--plain", "--silent",
         "--domain", metadata["domain"], "--projectId", project,
         "--env", metadata["environment"], "--path", path],
        capture_output=True, text=True,
    )
    value = result.stdout.strip()
    if result.returncode or not value or "\n" in value or "\r" in value:
        raise RuntimeError(f"Cannot load {name} from Infisical (details redacted)")
    return value


def credentials(mode, metadata):
    env = os.environ.copy()
    access, private = env.get("AWS_ACCESS_KEY_ID"), env.get("AWS_SECRET_ACCESS_KEY")
    if bool(access) != bool(private):
        raise RuntimeError("Provide both AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY")
    if not access and not env.get("AWS_PROFILE"):
        project = metadata["infraProjectId"] if mode == "push" else metadata["buildProjectId"]
        path = "/" if mode == "push" else metadata["cachePath"]
        for name in ("AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY"):
            env[name] = secret(name, project, path, metadata)
    env["AWS_EC2_METADATA_DISABLED"] = "true"
    return env


def store_path(item, env):
    if item.startswith("/nix/store/") or Path(item).exists():
        path = str(Path(item).resolve())
    else:
        path = run(["nix", "eval", "--raw", item + ".outPath"],
                   env=env, capture_output=True).stdout.strip()
    if not re.fullmatch(r"/nix/store/[a-z0-9]{32}-[^/]+", path):
        raise RuntimeError(f"Expected a Nix output path for {item}")
    return path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("pull", "push"))
    parser.add_argument("installables", nargs="*", default=[".#game", ".#game-deps"])
    parser.add_argument("--to-store", help="Isolated destination store for a pull validation")
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    if args.to_store and args.mode != "pull":
        parser.error("--to-store is only supported by pull")
    config = json.loads((ROOT / "nix/cache-config.json").read_text())
    metadata = json.loads((ROOT / "tools/infisical-config.json").read_text())
    public_key = (ROOT / "nix/cache-public-key").read_text().strip()
    env = credentials(args.mode, metadata)
    paths = [store_path(item, env) for item in args.installables]
    url = "s3://" + config["bucket"] + "?" + urlencode({
        "region": config["region"], "scheme": "https", "endpoint": config["endpoint"],
    })
    report = {"mode": args.mode, "bucket": config["bucket"], "paths": [],
              "misses": [], "unavailable": [], "public_key": public_key}
    trusted = ["--extra-trusted-public-keys", public_key]
    if args.mode == "push":
        key = env.pop(config["signingKeySecret"], None) or secret(
            config["signingKeySecret"], metadata["infraProjectId"], "/", metadata,
        )
        actual_public = run(["nix", "key", "convert-secret-to-public"],
                            input=key, capture_output=True, env=env).stdout.strip()
        if actual_public != public_key:
            raise RuntimeError("Signing key does not match nix/cache-public-key")
        with tempfile.TemporaryDirectory(prefix="parkour-nix-sign-") as directory:
            key_file = Path(directory) / "key"
            key_file.touch(mode=0o600)
            key_file.write_text(key)
            run(["nix", "store", "sign", "--recursive", "--key-file", str(key_file), *paths], env=env)
            run(["nix", "copy", *trusted, "--to", url, *paths], env=env)
        report["paths"] = paths
    else:
        # Explicit credentials are scoped to this client, not a system daemon.
        session = boto3.Session(
            aws_access_key_id=env.get("AWS_ACCESS_KEY_ID"),
            aws_secret_access_key=env.get("AWS_SECRET_ACCESS_KEY"),
            aws_session_token=env.get("AWS_SESSION_TOKEN"),
            profile_name=env.get("AWS_PROFILE"),
        )
        s3 = session.client("s3", endpoint_url="https://" + config["endpoint"],
                            region_name=config["region"],
                            config=Config(signature_version="s3v4", s3={"addressing_style": "virtual"}))
        # The reader deliberately cannot ListBucket. OVH also returns 403 for
        # an absent object in that case, so authenticate against a known object
        # before interpreting any per-output response. Never call a 403 a hit.
        try:
            s3.head_object(Bucket=config["bucket"], Key="nix-cache-info")
        except ClientError as error:
            status = error.response["ResponseMetadata"]["HTTPStatusCode"]
            raise RuntimeError(f"OVH Nix cache access check failed: HTTP {status}") from None
        for path in paths:
            narinfo = Path(path).name.split("-", 1)[0] + ".narinfo"
            try:
                s3.head_object(Bucket=config["bucket"], Key=narinfo)
            except ClientError as error:
                status = error.response["ResponseMetadata"]["HTTPStatusCode"]
                if status == 404:
                    print(f"OVH Nix cache miss: {path}")
                    report["misses"].append(path)
                    continue
                if status == 403:
                    print(f"OVH Nix entry unavailable (absent or denied, HTTP 403): {path}; build locally")
                    report["unavailable"].append({"path": path, "http_status": status})
                    continue
                raise RuntimeError(f"OVH Nix metadata read failed: HTTP {status}") from None
            destination = ["--to", args.to_store] if args.to_store else []
            run(["nix", "copy", *trusted, "--from", url, *destination, path], env=env)
            print(f"OVH Nix cache hit: {path}")
            report["paths"].append(path)
    if args.report:
        args.report.write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, subprocess.CalledProcessError) as error:
        print(f"Nix cache: {error}", file=sys.stderr)
        sys.exit(1)
    except BotoCoreError:
        print("Nix cache: network or AWS configuration failed (details redacted)", file=sys.stderr)
        sys.exit(1)
