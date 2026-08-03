#!/usr/bin/env python3
"""Refresh the `cached` fields of a DecompDeck catalog.

For every project it queries GitHub Releases (once), picks the release matching
the project's channel, and records:
  - cached.latest_tag / cached.published_at  -> drives "update available"
  - cached.platforms                         -> lets the app pre-filter by OS
                                                without hitting GitHub per repo

This mirrors src-tauri/src/github.rs so the app and the CI agree. Run it in the
catalog repo's CI (see .github/workflows/probe.yml) and commit the result.

Usage:
    GITHUB_TOKEN=ghp_xxx python3 tools/probe.py src-tauri/catalog.seed.json
"""
import json
import os
import re
import sys
import time
import urllib.request
import urllib.error

API = "https://api.github.com"


def gh_get(url, token):
    req = urllib.request.Request(
        url,
        headers={
            "User-Agent": "decompdeck-probe",
            "Accept": "application/vnd.github+json",
            "X-GitHub-Api-Version": "2022-11-28",
            **({"Authorization": f"Bearer {token}"} if token else {}),
        },
    )
    with urllib.request.urlopen(req, timeout=30) as r:
        return json.load(r)


def pick_release(releases, channel, rolling_tag):
    if channel == "rolling":
        for r in releases:
            if r.get("tag_name") == rolling_tag:
                return r
        return next((r for r in releases if not r.get("draft")), None)
    if channel == "prerelease":
        return next((r for r in releases if not r.get("draft")), None)
    stable = next(
        (r for r in releases if not r.get("draft") and not r.get("prerelease")), None
    )
    return stable or next((r for r in releases if not r.get("draft")), None)


def probe_project(p, token):
    repo = p["repo"]
    slug = f"{repo['owner']}/{repo['repo']}"
    try:
        releases = gh_get(f"{API}/repos/{slug}/releases?per_page=10", token)
    except urllib.error.HTTPError as e:
        print(f"  ! {slug}: HTTP {e.code}")
        return None
    if not isinstance(releases, list) or not releases:
        print(f"  ! {slug}: sin releases")
        return None
    rel = pick_release(releases, p.get("release_channel", "stable"), p.get("rolling_tag"))
    if not rel:
        print(f"  ! {slug}: sin release adecuada")
        return None

    names = [a["name"] for a in rel.get("assets", [])]
    platforms = [
        triple
        for triple, rx in p.get("asset_rules", {}).items()
        if any(re.search(rx, n) for n in names)
    ]
    cached = {
        "platforms": sorted(platforms),
        "latest_tag": rel.get("tag_name"),
        "published_at": rel.get("published_at"),
    }
    print(f"  ✓ {slug} @ {cached['latest_tag']}: {', '.join(platforms) or '—'}")
    return cached


def main():
    if len(sys.argv) < 2:
        print("uso: probe.py <catalog.json>")
        return 1
    path = sys.argv[1]
    token = os.environ.get("GITHUB_TOKEN")
    catalog = json.load(open(path))

    for p in catalog.get("projects", []):
        cached = probe_project(p, token)
        if cached is not None:
            p["cached"] = cached
        time.sleep(0.3)  # be gentle with the API

    catalog["source"] = "probed"
    json.dump(catalog, open(path, "w"), indent=2, ensure_ascii=False)
    open(path, "a").write("\n")
    print(f"catálogo actualizado: {path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
