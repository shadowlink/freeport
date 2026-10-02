#!/usr/bin/env python3
"""Download the curated set of CC-BY console models from Sketchfab into the
Freeport models dir, one per system, and write the attribution file.

Needs a Sketchfab API token (free account → https://sketchfab.com/settings/password):
    SKETCHFAB_TOKEN=xxxx tools/fetch_models.py [--dir DIR] [--only n64,psx]

Models are shortlisted for a coherent low-poly look and permissive licence
(CC Attribution). Edit MODELS to swap any of them; the app only needs
`<system>.glb` (or `<system>/scene.gltf`) in the models dir.
"""
import argparse, io, json, os, sys, urllib.request, zipfile
from pathlib import Path

# system id -> (sketchfab uid, title, author) — all CC Attribution, downloadable.
MODELS = {
    "n64": ("e4a5fd678bb946adaf8486b85793b9a9", "Nintendo 64", "zappygru"),
    "psx": ("d16d43889f2349d8b78711a40da03ce5", "PS1 low poly", "rave.Ar4ik"),
    "gb": ("35ce8524d55449b5ab9d6a587a0038ed", "Nintendo GAME BOY", "user12350"),
    "x360": ("3b8b0231e7214a148246635a03521727", "Xbox 360 FAT (low poly)", "senkinsky"),
    "pc": ("7ab9138fd0474a549fbff1be76b940c3", "Low-Poly Computer Case", "Ovenproofmeteor"),
    "gc": ("187a584e3bff46c2aa1bd8a701211fb4", "GameCube console", "8723516"),
    "gba": ("d645f514b3e64e0ead8253e8b58b31c4", "Game Boy Advance SP Slate", "(Sketchfab)"),
    "wii": ("b13a4c0289144aa9b436d8de72e6a7a0", "WII Console", "Twisty_z"),
    "nds": ("ca529eb7208746e89b7a28fd2246659d", "Nintendo DS Lite", "Cianon"),
    "ps2": ("99d0c0c7f024430ca1c6179a07d85ffa", "PlayStation 2", "temp0.crazy"),
    "psp": ("605b9202aa83410585e31d02a1d50972", "PlayStation Portable (low poly)", "senkinsky"),
    "xbox": ("d5f7df5462b44a6d8340bf27bbde3dfe", "Original Xbox Case Battlefront II", "(Sketchfab)"),
    "ps5": ("d788de3735964151a3e24fd59c0f1956", "PS5", "rtql8d"),
    # No good CC-BY candidates found yet: 3ds, dc (plaque fallback is used).
}

API = "https://api.sketchfab.com/v3"


def default_dir() -> Path:
    if sys.platform.startswith("win"):
        base = Path(os.environ.get("APPDATA", Path.home()))
    elif sys.platform == "darwin":
        base = Path.home() / "Library" / "Application Support"
    else:
        base = Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local" / "share"))
    return base / "decompdeck" / "models"


def get(url: str, token: str):
    req = urllib.request.Request(url, headers={"Authorization": f"Token {token}", "User-Agent": "freeport-fetch-models"})
    with urllib.request.urlopen(req, timeout=60) as r:
        return r.read()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dir", type=Path, default=default_dir())
    ap.add_argument("--only", default="", help="comma-separated system ids")
    args = ap.parse_args()
    token = os.environ.get("SKETCHFAB_TOKEN", "").strip()
    if not token:
        sys.exit("falta SKETCHFAB_TOKEN (token de API de tu cuenta de Sketchfab)")
    args.dir.mkdir(parents=True, exist_ok=True)
    only = {s for s in args.only.split(",") if s}
    credits = []
    for sys_id, (uid, title, author) in MODELS.items():
        if only and sys_id not in only:
            continue
        info = json.loads(get(f"{API}/models/{uid}", token))
        lic = (info.get("license") or {}).get("label", "?")
        user = (info.get("user") or {}).get("username", author)
        dl = json.loads(get(f"{API}/models/{uid}/download", token))
        if "glb" in dl:
            data = get(dl["glb"]["url"], token)
            (args.dir / f"{sys_id}.glb").write_bytes(data)
            how = f"{sys_id}.glb"
        elif "gltf" in dl:
            data = get(dl["gltf"]["url"], token)
            out = args.dir / sys_id
            out.mkdir(exist_ok=True)
            zipfile.ZipFile(io.BytesIO(data)).extractall(out)
            how = f"{sys_id}/scene.gltf"
        else:
            print(f"  {sys_id}: sin formato descargable", file=sys.stderr)
            continue
        print(f"  {sys_id}: {info.get('name', title)} por {user} ({lic}) → {how}")
        credits.append(f"{sys_id}: \"{info.get('name', title)}\" by {user} — {lic} — https://sketchfab.com/3d-models/{uid}")
    if credits:
        (args.dir / "CREDITS.txt").write_text(
            "Modelos 3D de consolas usados por el modo inmersivo de Freeport.\n"
            "Licencia Creative Commons Attribution: se requiere citar al autor.\n\n" + "\n".join(credits) + "\n"
        )
        print(f"Atribución escrita en {args.dir / 'CREDITS.txt'}")


if __name__ == "__main__":
    main()
