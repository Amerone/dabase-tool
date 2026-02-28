#!/usr/bin/env python3
"""Download Windows SDK x64 libs from NuGet packages (no admin required)."""

import urllib.request
import zipfile
import os
import sys

OUTPUT_DIR = os.path.expanduser("~/.winsdk")
os.makedirs(OUTPUT_DIR, exist_ok=True)

# NuGet packages that contain Windows SDK x64 libs
PACKAGES = [
    {
        "id": "Microsoft.Windows.SDK.CPP",
        "version": "10.0.22621.755",
        "url": "https://www.nuget.org/api/v2/package/Microsoft.Windows.SDK.CPP/10.0.22621.755",
    },
    {
        "id": "Microsoft.Windows.SDK.CPP.x64",
        "version": "10.0.22621.755",
        "url": "https://www.nuget.org/api/v2/package/Microsoft.Windows.SDK.CPP.x64/10.0.22621.755",
    },
]

def download_package(pkg):
    name = pkg["id"]
    version = pkg["version"]
    url = pkg["url"]
    nupkg_path = os.path.join(OUTPUT_DIR, f"{name}.{version}.nupkg")
    extract_dir = os.path.join(OUTPUT_DIR, f"{name}")

    if os.path.exists(extract_dir) and os.listdir(extract_dir):
        print(f"[skip] {name} already extracted")
        return extract_dir

    if not os.path.exists(nupkg_path):
        print(f"[download] {name} {version} ...")
        req = urllib.request.Request(url, headers={"User-Agent": "nuget"})
        with urllib.request.urlopen(req, timeout=120) as response:
            total = int(response.headers.get("Content-Length", 0))
            downloaded = 0
            chunk_size = 65536
            with open(nupkg_path, "wb") as f:
                while True:
                    chunk = response.read(chunk_size)
                    if not chunk:
                        break
                    f.write(chunk)
                    downloaded += len(chunk)
                    if total:
                        pct = downloaded * 100 // total
                        print(f"\r  {pct}% ({downloaded // 1024}KB / {total // 1024}KB)", end="", flush=True)
            print()
        print(f"  -> saved to {nupkg_path}")

    print(f"[extract] {name} ...")
    os.makedirs(extract_dir, exist_ok=True)
    with zipfile.ZipFile(nupkg_path, "r") as zf:
        zf.extractall(extract_dir)
    print(f"  -> extracted to {extract_dir}")
    return extract_dir

lib_paths = []
for pkg in PACKAGES:
    try:
        d = download_package(pkg)
        # Look for x64 lib files
        for root, dirs, files in os.walk(d):
            if any(f.endswith(".lib") for f in files):
                if "x64" in root or "um" in root.lower() or "ucrt" in root.lower():
                    lib_paths.append(root)
    except Exception as e:
        print(f"ERROR: {e}", file=sys.stderr)
        sys.exit(1)

# Deduplicate
lib_paths = list(dict.fromkeys(lib_paths))
print("\n[result] Library paths:")
for p in lib_paths:
    print(f"  {p}")

# Write a file that cargo can use
config_path = os.path.join(OUTPUT_DIR, "lib_paths.txt")
with open(config_path, "w") as f:
    f.write(";".join(lib_paths))
print(f"\nWritten to {config_path}")
print("Set LIB environment variable to:")
print(";".join(lib_paths))
