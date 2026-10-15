#!/usr/bin/env python3
"""Ensure a crates.io trusted publisher record exists for each crate we publish.

Trusted publishing replaces a long-lived CARGO_REGISTRY_TOKEN with a short-lived
OIDC token that crates.io mints for one workflow, repository and environment.
The crates.io API can only create a record with a token that carries the
trusted-publishing scope, so this runs inside a workflow that still holds one;
once every record exists, the token is revoked and no code path reads it again.

Idempotent: a crate whose record already exists is left alone. The create call
is not retried, because it is not idempotent on the registry side.

Usage:
  scripts/ensure-trusted-publishers.py --crate NAME [--crate NAME ...] \
      --owner SylphxAI --repo REPO --workflow FILE [--environment NAME]
"""

import argparse
import json
import os
import sys
import urllib.error
import urllib.request

API = "https://crates.io/api/v1/trusted_publishing/github_configs"


USER_AGENT = "SylphxAI-trusted-publishing/1.0 (+https://github.com/SylphxAI)"


def call(method, url, token, payload=None):
    body = json.dumps(payload).encode() if payload is not None else None
    request = urllib.request.Request(url, data=body, method=method)
    request.add_header("Authorization", f"Bearer {token}")
    request.add_header("User-Agent", USER_AGENT)
    if body is not None:
        request.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            raw = response.read()
            return response.status, (json.loads(raw) if raw else None)
    except urllib.error.HTTPError as error:
        raw = error.read()
        try:
            return error.code, json.loads(raw) if raw else None
        except ValueError:
            return error.code, None


def existing(url, token, crate):
    status, doc = call("GET", f"{url}?crate={crate}", token)
    if status != 200:
        raise RuntimeError(f"listing trusted publishers for {crate} failed: HTTP {status} {doc}")
    configs = (doc or {}).get("github_configs")
    if not isinstance(configs, list):
        raise RuntimeError(f"listing trusted publishers for {crate} is not a config list: {doc}")
    return configs


def matches(config, crate, owner, repo, workflow, environment):
    return (
        config.get("crate") == crate
        and config.get("repository_owner") == owner
        and config.get("repository_name") == repo
        and config.get("workflow_filename") == workflow
        and (config.get("environment") or "") == (environment or "")
    )


def ensure(url, token, crate, owner, repo, workflow, environment):
    configs = existing(url, token, crate)
    if any(matches(c, crate, owner, repo, workflow, environment) for c in configs):
        return "present"
    payload = {
        "github_config": {
            "crate": crate,
            "repository_owner": owner,
            "repository_name": repo,
            "workflow_filename": workflow,
            "environment": environment or None,
        }
    }
    status, doc = call("POST", url, token, payload)
    if status not in (200, 201):
        raise RuntimeError(f"creating the trusted publisher for {crate} failed: HTTP {status} {doc}")
    configs = existing(url, token, crate)
    if not any(matches(c, crate, owner, repo, workflow, environment) for c in configs):
        raise RuntimeError(f"the trusted publisher for {crate} is not readable after creation")
    return "created"


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--crate", action="append", required=True, help="a crate to configure; repeatable")
    parser.add_argument("--owner", required=True)
    parser.add_argument("--repo", required=True)
    parser.add_argument("--workflow", required=True)
    parser.add_argument("--environment", default="")
    parser.add_argument("--url", default=API)
    args = parser.parse_args(argv)

    token = os.environ.get("CARGO_REGISTRY_TOKEN")
    if not token:
        print("CARGO_REGISTRY_TOKEN is empty; it must carry the crates.io trusted-publishing scope",
              file=sys.stderr)
        return 1

    failed = []
    for crate in args.crate:
        try:
            result = ensure(args.url, token, crate, args.owner, args.repo, args.workflow, args.environment)
        except RuntimeError as error:
            print(f"{crate}: {error}", file=sys.stderr)
            failed.append(crate)
            continue
        print(f"{crate}: trusted publisher {result} for {args.owner}/{args.repo} {args.workflow}"
              f"{' [' + args.environment + ']' if args.environment else ''}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
