#!/usr/bin/env bash
set -euo pipefail

# Fetches the Tabler icons referenced by assets/icons/icons.toml into assets/icons/tabler/.
#
#   scripts/sync-tabler-icons.sh            # download missing icons, prune unused ones
#   scripts/sync-tabler-icons.sh --check    # only verify; exit 1 if anything is missing or unused
#
# The icons come from the official `@tabler/icons` npm package (MIT license). The package version and its
# integrity hash are pinned in the manifest, the tarball is verified before anything is extracted, and the
# original Tabler file names are preserved (assets/icons/tabler/<outline|filled>/<tabler-name>.svg).
# Requires: curl, tar, openssl, base64, awk.

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MANIFEST="${ROOT}/assets/icons/icons.toml"
DEST="${ROOT}/assets/icons/tabler"
MODE="sync"
[[ "${1:-}" == "--check" ]] && MODE="check"

VERSION="$(awk -F'"' '/^tabler_version/ {print $2}' "${MANIFEST}")"
INTEGRITY="$(awk -F'"' '/^tabler_integrity/ {print $2}' "${MANIFEST}")"
if [[ -z "${VERSION}" || -z "${INTEGRITY}" ]]; then
    echo "[-] tabler_version / tabler_integrity missing in ${MANIFEST}" >&2
    exit 1
fi

# Wanted files as "<style>/<name>.svg" (one per line, unique), parsed from the single-line [icons] entries.
WANTED="$(awk '
    /^\[icons\]/ { in_icons = 1; next }
    /^\[/        { in_icons = 0 }
    in_icons && /tabler *= *"/ {
        name = ""; style = "outline"
        if (match($0, /tabler *= *"[^"]+"/)) { name = substr($0, RSTART, RLENGTH); sub(/^[^"]*"/, "", name); sub(/"$/, "", name) }
        if (match($0, /style *= *"[^"]+"/))  { style = substr($0, RSTART, RLENGTH); sub(/^[^"]*"/, "", style); sub(/"$/, "", style) }
        print style "/" name ".svg"
    }' "${MANIFEST}" | sort -u)"

MISSING=()
while IFS= read -r rel; do
    [[ -f "${DEST}/${rel}" ]] || MISSING+=("${rel}")
done <<< "${WANTED}"

UNUSED=()
if [[ -d "${DEST}" ]]; then
    while IFS= read -r file; do
        rel="${file#"${DEST}"/}"
        grep -qxF "${rel}" <<< "${WANTED}" || UNUSED+=("${rel}")
    done < <(find "${DEST}" -type f -name '*.svg' | sort)
fi

if [[ "${MODE}" == "check" ]]; then
    for m in ${MISSING[@]+"${MISSING[@]}"}; do echo "missing: ${m}"; done
    for u in ${UNUSED[@]+"${UNUSED[@]}"}; do echo "unused:  ${u}"; done
    [[ ${#MISSING[@]} -eq 0 && ${#UNUSED[@]} -eq 0 ]] && echo "[+] Tabler icons are in sync with the manifest." && exit 0
    exit 1
fi

if [[ ${#MISSING[@]} -gt 0 || ! -f "${DEST}/LICENSE" ]]; then
    WORK="$(mktemp -d)"
    trap 'rm -rf "${WORK}"' EXIT
    URL="https://registry.npmjs.org/@tabler/icons/-/icons-${VERSION}.tgz"
    echo "==> Downloading ${URL}"
    curl -fsSL -o "${WORK}/icons.tgz" "${URL}"

    ACTUAL="sha512-$(openssl dgst -sha512 -binary "${WORK}/icons.tgz" | base64 -w0)"
    if [[ "${ACTUAL}" != "${INTEGRITY}" ]]; then
        echo "[-] Integrity mismatch for @tabler/icons@${VERSION}" >&2
        echo "    expected ${INTEGRITY}" >&2
        echo "    actual   ${ACTUAL}" >&2
        exit 1
    fi

    mkdir -p "${WORK}/pkg"
    tar -xzf "${WORK}/icons.tgz" -C "${WORK}/pkg"
    PKG="${WORK}/pkg/package"

    for rel in ${MISSING[@]+"${MISSING[@]}"}; do
        src="${PKG}/icons/${rel}"
        if [[ ! -f "${src}" ]]; then
            echo "[-] Tabler ${VERSION} has no icon '${rel}'" >&2
            exit 1
        fi
        mkdir -p "$(dirname "${DEST}/${rel}")"
        cp "${src}" "${DEST}/${rel}"
        echo "    + ${rel}"
    done
    cp "${PKG}/LICENSE" "${DEST}/LICENSE"
fi

for u in ${UNUSED[@]+"${UNUSED[@]}"}; do
    rm -f "${DEST}/${u}"
    echo "    - ${u}"
done
find "${DEST}" -type d -empty -delete 2>/dev/null || true

echo "[+] Tabler ${VERSION}: $(grep -c . <<< "${WANTED}") icons in assets/icons/tabler"
