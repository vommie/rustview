#!/usr/bin/env bash
set -euo pipefail

# RustView Arch Linux package builder
#
# Run from the project root:
#
#   ./build-package.sh
#
# Expected project layout:
#
#   rustview/
#   ├── Cargo.toml
#   ├── Cargo.lock
#   ├── LICENSE
#   ├── build-package.sh
#   ├── target/
#   │   └── release/
#   │       └── rustview
#   └── packaging/
#       ├── PKGBUILD
#       ├── rustview.desktop
#       └── rustview.svg

PROJECT_ROOT="$(pwd)"
PACKAGING_DIR="${PROJECT_ROOT}/packaging"
PKGBUILD="${PACKAGING_DIR}/PKGBUILD"
DIST_DIR="${PROJECT_ROOT}/dist"
BINARY="${PROJECT_ROOT}/target/release/rustview"
CARGO_TOML="${PROJECT_ROOT}/Cargo.toml"

echo "==> [RustView Packager] Starting Arch Linux package build..."

# ---------------------------------------------------------------------------
# 1. Verify required files
# ---------------------------------------------------------------------------

for file in \
    "${CARGO_TOML}" \
    "${PROJECT_ROOT}/Cargo.lock" \
    "${PROJECT_ROOT}/LICENSE" \
    "${BINARY}" \
    "${PKGBUILD}" \
    "${PACKAGING_DIR}/rustview.desktop" \
    "${PACKAGING_DIR}/rustview.svg"
do
    if [[ ! -f "${file}" ]]; then
        echo "[-] Error: Required file not found: ${file}" >&2
        exit 1
    fi
done

if [[ ! -x "${BINARY}" ]]; then
    echo "[-] Error: Release binary is not executable: ${BINARY}" >&2
    exit 1
fi

# ---------------------------------------------------------------------------
# 2. Read version from [package] in Cargo.toml
# ---------------------------------------------------------------------------

VERSION="$(
    python - "${CARGO_TOML}" <<'PY'
import re
import sys

with open(sys.argv[1], encoding="utf-8") as f:
    cargo_toml = f.read()

match = re.search(
    r'(?ms)^\[package\]\s*(.*?)(?=^\[|\Z)',
    cargo_toml,
)

if not match:
    raise SystemExit("Could not find [package] section")

package_section = match.group(1)

match = re.search(
    r'(?m)^version\s*=\s*"([^"]+)"\s*$',
    package_section,
)

if not match:
    raise SystemExit("Could not find package version")

print(match.group(1))
PY
)"

if [[ -z "${VERSION}" ]]; then
    echo "[-] Error: Could not determine version from Cargo.toml." >&2
    exit 1
fi

echo "[+] Detected version: ${VERSION}"

# ---------------------------------------------------------------------------
# 3. Synchronize pkgver
# ---------------------------------------------------------------------------

if ! grep -q '^pkgver=' "${PKGBUILD}"; then
    echo "[-] Error: PKGBUILD does not contain pkgver=." >&2
    exit 1
fi

sed -i "s/^pkgver=.*/pkgver=${VERSION}/" "${PKGBUILD}"

echo "[+] PKGBUILD version synchronized: ${VERSION}"

# ---------------------------------------------------------------------------
# 4. Prepare dist/
# ---------------------------------------------------------------------------

mkdir -p "${DIST_DIR}"

find "${DIST_DIR}" \
    -maxdepth 1 \
    -type f \
    -name 'rustview-*.pkg.tar.*' \
    -delete

# ---------------------------------------------------------------------------
# 5. Clean previous makepkg artifacts
# ---------------------------------------------------------------------------

rm -rf \
    "${PACKAGING_DIR}/pkg" \
    "${PACKAGING_DIR}/src"

find "${PACKAGING_DIR}" \
    -maxdepth 1 \
    -type f \
    -name 'rustview-*.pkg.tar.*' \
    -delete

# ---------------------------------------------------------------------------
# 6. Validate PKGBUILD if namcap is available
# ---------------------------------------------------------------------------

if command -v namcap >/dev/null 2>&1; then
    echo "[+] Checking PKGBUILD with namcap..."
    namcap "${PKGBUILD}"
fi

# ---------------------------------------------------------------------------
# 7. Build package
# ---------------------------------------------------------------------------

echo "[+] Invoking makepkg..."

(
    cd "${PACKAGING_DIR}"
    makepkg --force --clean --log
)

# ---------------------------------------------------------------------------
# 8. Find generated package
# ---------------------------------------------------------------------------

mapfile -t PACKAGES < <(
    find "${PACKAGING_DIR}" \
        -maxdepth 1 \
        -type f \
        -name "rustview-${VERSION}-*.pkg.tar.*" \
        -print
)

if (( ${#PACKAGES[@]} != 1 )); then
    echo "[-] Error: Expected exactly one generated package, found ${#PACKAGES[@]}." >&2

    if (( ${#PACKAGES[@]} > 0 )); then
        printf '    %s\n' "${PACKAGES[@]}"
    fi

    exit 1
fi

PKG_FILE="${PACKAGES[0]}"
PKG_NAME="$(basename "${PKG_FILE}")"
FINAL_PKG="${DIST_DIR}/${PKG_NAME}"

mv -- "${PKG_FILE}" "${FINAL_PKG}"

# ---------------------------------------------------------------------------
# 9. Validate generated package
# ---------------------------------------------------------------------------

if command -v namcap >/dev/null 2>&1; then
    echo "[+] Checking generated package with namcap..."
    namcap "${FINAL_PKG}"
fi

# ---------------------------------------------------------------------------
# 10. Done
# ---------------------------------------------------------------------------

echo
echo "============================================================"
echo "[✓] RustView Arch package built successfully!"
echo "    Version: ${VERSION}"
echo "    Package: ${FINAL_PKG}"
echo "============================================================"
echo
echo "Install locally:"
echo "    sudo pacman -U \"${FINAL_PKG}\""
echo
echo "Transfer to another Arch system:"
echo "    scp \"${FINAL_PKG}\" user@remote-pc:/tmp/"
echo
