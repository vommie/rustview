#!/usr/bin/env python3
"""Writes ATTRIBUTION.md: only what the licenses require when a built RustView is passed on.

    scripts/attribution.py           regenerate the file
    scripts/attribution.py --check   fail when the file is stale

The MIT, BSD, ISC and Unicode licenses ask to keep the copyright lines and the license text with every copy, also in a binary, so
each text stands once with its lines; the crates' names are not required. Zlib, BSL-1.0, 0BSD, Unlicense, WTFPL and CC0 ask nothing
of a binary. Apache-2.0 asks for its text (`LICENSES/`) and no crate here has a NOTICE file (the check fails when one gets one). GPL,
LGPL and MPL ask to name the work, its license and where its source is: that list is made from the crates. C and C++ code that crates
compile in is not seen by cargo-about: `BUNDLED` holds what it needs, checked for the crate versions named there.
"""
import collections
import json
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
TARGET = ROOT / "ATTRIBUTION.md"
# The licenses that ask for copyright lines and their text in a binary.
REQUIRED = ("BSD-2-Clause", "BSD-3-Clause", "ISC", "MIT", "Unicode-3.0")
# The licenses that ask to name the work and where its source is.
COPYLEFT = {"GPL-3.0-only": "GPL-3.0-only.txt", "LGPL-2.1": "LGPL-2.1-only.txt", "MPL-2.0": "MPL-2.0.txt"}
# Crates that compile in C or C++ code, at the version whose code was looked at, and what that code asks for (the head names them).
# A new version fails the check: look at what it compiles in, update the lines and the head, then the version here.
BUNDLED = {
    # Skia's prebuilt library (features ganesh, gl, jpeg, pdf, vulkan) holds libpng, zlib, libjpeg-turbo (libjpeg API only),
    # Expat and Wuffs; FreeType, fontconfig and ICU are not in it.
    "skia-bindings": "0.153.3",
    "libwebp-sys": "0.9.6",
    "lmdb-master-sys": "0.2.6",
    # SQLite is in the public domain.
    "libsqlite3-sys": "0.38.2",
}
EXTRA_LINES = {
    "MIT": (
        "Copyright (c) 2020-2026 Paweł Kuna",
        "Copyright (c) The Rust Project Developers",
        "Copyright (c) 1998-2000 Thai Open Source Software Center Ltd and Clark Cooper",
        "Copyright (c) 2001-2025 Expat maintainers",
    ),
    "BSD-3-Clause": ("Copyright (c) 2011 Google Inc. All rights reserved.", "Copyright (c) 2010, Google Inc. All rights reserved."),
    "ISC": ("Copyright (c) 2009, 2010 Martin Hedenfalk <martin@bzero.se>",),
}
LMDB_LINES = ("Copyright 2011-2021 Howard Chu, Symas Corp. All rights reserved.", "Copyright 2000-2021 The OpenLDAP Foundation.")
COPYRIGHT = re.compile(r"^\W*(copyright\b|©|\(c\)\s*\d)", re.IGNORECASE)
PLACEHOLDER = re.compile(r"[\[{](yyyy|year|name|fullname)|<(year|copyright|owner|name)|name of copyright owner|copyright holders?\s*$|^\W*copyright (notice|and|law|holder)", re.IGNORECASE)


def holders(text):
    found = (line.strip(" \t*#/") for line in text.splitlines() if COPYRIGHT.match(line))
    return sorted({re.sub(r"\s+", " ", line) for line in found if not PLACEHOLDER.search(line)})


def body(text):
    """The license text without its copyright lines."""
    kept = [line.rstrip() for line in text.splitlines() if not COPYRIGHT.match(line) or PLACEHOLDER.search(line)]
    return re.sub(r"\n{3,}", "\n\n", "\n".join(kept)).strip()


def own(use):
    return use["crate"]["name"].startswith("rustview")


def about():
    raw = subprocess.check_output(
        ["cargo", "about", "generate", "--locked", "--workspace", "--format", "json"], cwd=ROOT, stderr=subprocess.DEVNULL)
    return json.loads(raw)


def bundled_sources(data):
    """The source folder of each crate in `BUNDLED`; fails when its version is not the one that was looked at."""
    found = {}
    for entry in data["crates"]:
        package = entry["package"]
        if package["name"] in BUNDLED:
            if package["version"] != BUNDLED[package["name"]]:
                sys.exit(f'{package["name"]} is now {package["version"]}: look at the C code it compiles in, then update BUNDLED in '
                         "scripts/attribution.py and scripts/attribution-head.md")
            found[package["name"]] = pathlib.Path(package["manifest_path"]).parent
    missing = sorted(set(BUNDLED) - set(found))
    if missing:
        sys.exit(f"no longer in the build: {', '.join(missing)}; remove them from BUNDLED and the head")
    return found


def copyleft(data):
    """One line per repository of crates under a license that asks to be named."""
    groups = collections.defaultdict(set)
    for license in data["licenses"]:
        if license["id"] in COPYLEFT:
            for use in license["used_by"]:
                if not own(use):
                    repo = (use["crate"]["repository"] or "").removesuffix(".git")
                    groups[(license["id"], repo)].add(use["crate"]["name"])
    out = []
    for (key, repo), names in sorted(groups.items(), key=lambda item: min(item[1], key=len)):
        if len(names) <= 3:
            named = " and ".join(f"**{name}**" for name in sorted(names, key=len))
        else:
            named = f"**{min(names, key=len)}** and {len(names) - 1} more crates of its repository"
        out.append(f"- {named}, compiled in, under the {key} (`LICENSES/{COPYLEFT[key]}`). Source: <{repo}>.")
    return out


def notices(data):
    for license in data["licenses"]:
        if license["id"] == "Apache-2.0":
            for use in license["used_by"]:
                folder = pathlib.Path(use["crate"]["manifest_path"]).parent
                if any(path.name.upper().startswith("NOTICE") for path in folder.iterdir()):
                    sys.exit(f'{use["crate"]["name"]} has a NOTICE file: add it to scripts/attribution.py')


def collect(data, sources):
    groups = collections.defaultdict(lambda: {"name": "", "texts": collections.Counter(), "lines": set()})
    for license in data["licenses"]:
        key = license["id"]
        if key not in REQUIRED or all(own(use) for use in license["used_by"]):
            continue
        groups[key]["name"] = license["name"].replace("&quot;", '"')
        groups[key]["texts"][body(license["text"])] += 1
        groups[key]["lines"].update(holders(license["text"]))
    for key, lines in EXTRA_LINES.items():
        groups[key]["lines"].update(lines)
    groups["BSD-3-Clause"]["texts"][body((sources["libwebp-sys"] / "vendor/COPYING").read_text())] += 1
    return groups


def render():
    data = about()
    sources = bundled_sources(data)
    notices(data)
    out = [(ROOT / "scripts/attribution-head.md").read_text().rstrip(), "", "## Named because the license asks for it", ""]
    out += copyleft(data)
    out.append("")
    for key, group in sorted(collect(data, sources).items()):
        out += [f'## {group["name"]}', ""]
        out += [f"- {line}" for line in sorted(group["lines"], key=str.lower)]
        out.append("")
        for text, _ in group["texts"].most_common(1 if key in ("MIT", "ISC") else None):
            out += ["```", text, "```", ""]
    lmdb = (sources["lmdb-master-sys"] / "lmdb/libraries/liblmdb/LICENSE").read_text().strip()
    out += ["## The OpenLDAP Public License 2.8 (LMDB)", ""]
    out += [f"- {line}" for line in LMDB_LINES]
    out += ["", "```", lmdb, "```", ""]
    return "\n".join(out).rstrip() + "\n"


def main():
    text = render()
    if sys.argv[1:] == ["--check"]:
        if not TARGET.exists() or TARGET.read_text() != text:
            sys.exit("ATTRIBUTION.md is stale: run scripts/attribution.py")
        return
    TARGET.write_text(text)
    print(f"ATTRIBUTION.md: {len(text) // 1024} KB")


if __name__ == "__main__":
    main()
