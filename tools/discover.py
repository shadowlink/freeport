#!/usr/bin/env python3
"""Discover launchable-decomp candidates from the community awesome-lists.

The two community lists are plain Markdown (`- [Name](repo-url)`) with ~1000
entries, most of which are *matching decompilations that ship no binary*. This
script parses them, keeps GitHub repos, and (optionally) checks which ones
publish Releases with downloadable assets — i.e. the small subset that is
actually launchable and worth hand-curating into catalog.json.

It does NOT write the catalog; it emits candidates for a human to review, so the
curated metadata (system, asset_rules, ROM rules, art) stays deliberate.

Usage:
    GITHUB_TOKEN=ghp_xxx python3 tools/discover.py --check > candidates.json
"""
import argparse
import json
import os
import re
import sys
import time
import urllib.request
import urllib.error

LISTS = {
    # CharlotteCross groups by console (section headers); SamidyFR is an
    # alphabetical superset auto-synced from it daily.
    "charlotte": "https://raw.githubusercontent.com/CharlotteCross1998/awesome-game-decompilations/main/README.md",
    "samidy": "https://raw.githubusercontent.com/SamidyFR/Game-Decompilations/main/README.md",
}
ENTRY_RE = re.compile(r"-\s*\[(.+?)\]\((https?://[^)]+)\)")
GH_RE = re.compile(r"github\.com/([^/]+)/([^/#?]+)")


def fetch_text(url):
    req = urllib.request.Request(url, headers={"User-Agent": "decompdeck-discover"})
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            return r.read().decode("utf-8", "replace")
    except urllib.error.HTTPError:
        # Some repos default to `master`.
        alt = url.replace("/main/", "/master/")
        with urllib.request.urlopen(
            urllib.request.Request(alt, headers={"User-Agent": "decompdeck-discover"}),
            timeout=30,
        ) as r:
            return r.read().decode("utf-8", "replace")


def parse_entries(md):
    out = {}
    for name, url in ENTRY_RE.findall(md):
        m = GH_RE.search(url)
        if not m:
            continue  # skip GitLab/Codeberg/SourceHut for the release check
        owner, repo = m.group(1), m.group(2).removesuffix(".git")
        out[f"{owner}/{repo}"] = name.strip()
    return out


def has_binary_release(slug, token):
    url = f"https://api.github.com/repos/{slug}/releases?per_page=3"
    req = urllib.request.Request(
        url,
        headers={
            "User-Agent": "decompdeck-discover",
            "Accept": "application/vnd.github+json",
            **({"Authorization": f"Bearer {token}"} if token else {}),
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            data = json.load(r)
    except urllib.error.HTTPError:
        return False
    return any(rel.get("assets") for rel in data if isinstance(rel, dict))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true",
                    help="query GitHub Releases to keep only repos with binaries")
    ap.add_argument("--limit", type=int, default=0,
                    help="cap release checks (rate limit: 60/h without a token)")
    args = ap.parse_args()
    token = os.environ.get("GITHUB_TOKEN")

    entries = {}
    for key, url in LISTS.items():
        md = fetch_text(url)
        found = parse_entries(md)
        print(f"# {key}: {len(found)} repos de GitHub", file=sys.stderr)
        entries.update(found)
    print(f"# total únicos: {len(entries)}", file=sys.stderr)

    if not args.check:
        json.dump(
            [{"repo": s, "name": n} for s, n in sorted(entries.items())],
            sys.stdout,
            indent=2,
            ensure_ascii=False,
        )
        return 0

    candidates = []
    checked = 0
    for slug, name in sorted(entries.items()):
        if args.limit and checked >= args.limit:
            print(f"# límite de {args.limit} comprobaciones alcanzado", file=sys.stderr)
            break
        checked += 1
        if has_binary_release(slug, token):
            candidates.append({"repo": slug, "name": name})
            print(f"  + {slug} ({name})", file=sys.stderr)
        time.sleep(0.3)

    json.dump(candidates, sys.stdout, indent=2, ensure_ascii=False)
    print(f"\n# {len(candidates)} candidatos con binario", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
