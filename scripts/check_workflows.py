#!/usr/bin/env python3
"""Parses every workflow file.

A malformed workflow does not fail anything: GitHub registers it by path instead of by name,
reports no triggers, and silently never runs. That is how `run: echo "a: b"` -- a colon inside an
unquoted scalar -- cost a release step here. This turns it into a test failure instead.
"""
import pathlib
import sys

try:
    import yaml
except ImportError:  # the check is advisory when PyYAML is absent
    print("PyYAML not installed; skipping workflow syntax check")
    sys.exit(0)

status = 0
for path in sorted(pathlib.Path(".github/workflows").glob("*.yml")):
    try:
        document = yaml.safe_load(path.read_text())
    except yaml.YAMLError as error:
        print(f"{path}: {error}", file=sys.stderr)
        status = 1
        continue
    if not isinstance(document, dict):
        print(f"{path}: not a mapping", file=sys.stderr)
        status = 1
        continue
    # PyYAML resolves a bare `on:` key to the boolean True.
    triggers = document.get("on", document.get(True))
    if not triggers:
        print(f"{path}: no triggers; GitHub would never run this", file=sys.stderr)
        status = 1
        continue
    if not document.get("jobs"):
        print(f"{path}: no jobs", file=sys.stderr)
        status = 1
        continue
    print(f"  ok  {path.name}: {', '.join(triggers)}")
sys.exit(status)
