#!/usr/bin/env bash
# RustView Arch Linux package builder.
#
# Run from the project root:
#
#   ./build-package.sh
#
# Builds the release binary (cargo build --release --locked), synchronizes pkgver in packaging/PKGBUILD with the workspace version in Cargo.toml, runs makepkg
# and namcap, and leaves dist/rustview-<version>-1-x86_64.pkg.tar.zst (install with `sudo pacman -U`).
#
#   rustview/
#   ├── Cargo.toml, Cargo.lock, LICENSE, ATTRIBUTION.md, LICENSES/, README.md, CHANGELOG.md
#   ├── target/release/rustview        (built by this script)
#   └── packaging/
#       ├── PKGBUILD
#       ├── io.github.vommie.rustview.desktop
#       ├── io.github.vommie.rustview.metainfo.xml
#       ├── rustview.svg, rustview-symbolic.svg
#       └── icons/rustview-<size>.png
set -euo pipefail
cd "$(dirname "$0")"

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
    "${PROJECT_ROOT}/ATTRIBUTION.md" \
    "${PROJECT_ROOT}/LICENSES/GPL-3.0-only.txt" \
    "${PROJECT_ROOT}/README.md" \
    "${PROJECT_ROOT}/CHANGELOG.md" \
    "${PKGBUILD}" \
    "${PACKAGING_DIR}/io.github.vommie.rustview.desktop" \
    "${PACKAGING_DIR}/io.github.vommie.rustview.metainfo.xml" \
    "${PACKAGING_DIR}/rustview.svg" \
    "${PACKAGING_DIR}/rustview-symbolic.svg"
do
    if [[ ! -f "${file}" ]]; then
        echo "[-] Error: Required file not found: ${file}" >&2
        exit 1
    fi
done

for size in 16 22 24 32 48 64 128 256 512; do
    if [[ ! -f "${PACKAGING_DIR}/icons/rustview-${size}.png" ]]; then
        echo "[-] Error: icon size ${size} is missing; run scripts/gen-app-icons.sh" >&2
        exit 1
    fi
done

# ---------------------------------------------------------------------------
# 2. Read version from [workspace.package] in Cargo.toml (the first version line of the file)
# ---------------------------------------------------------------------------

VERSION="$(sed -n 's/^version *= *"\(.*\)"/\1/p' "${CARGO_TOML}" | head -n 1)"

if [[ -z "${VERSION}" ]]; then
    echo "[-] Error: Could not determine version from Cargo.toml." >&2
    exit 1
fi

echo "[+] Detected version: ${VERSION}"

# ---------------------------------------------------------------------------
# 3. Build the release binary, so the package always holds the current tree
# ---------------------------------------------------------------------------

echo "[+] Building the release binary..."
cargo build --release --locked

if [[ ! -x "${BINARY}" ]]; then
    echo "[-] Error: Release binary is missing or not executable: ${BINARY}" >&2
    exit 1
fi

# ---------------------------------------------------------------------------
# 4. Synchronize pkgver and validate the desktop integration files
# ---------------------------------------------------------------------------

sed -i "s/^pkgver=.*/pkgver=${VERSION}/" "${PKGBUILD}"
echo "[+] PKGBUILD version synchronized: ${VERSION}"

if command -v desktop-file-validate >/dev/null 2>&1; then
    desktop-file-validate "${PACKAGING_DIR}/io.github.vommie.rustview.desktop"
    echo "[+] Desktop file is valid"
fi

if command -v appstreamcli >/dev/null 2>&1; then
    METAINFO_CHECK="$(mktemp --suffix=.metainfo.xml)"
    sed -e "s/@VERSION@/${VERSION}/" -e "s/@DATE@/$(date -u +%F)/" \
        "${PACKAGING_DIR}/io.github.vommie.rustview.metainfo.xml" > "${METAINFO_CHECK}"
    appstreamcli validate --no-net "${METAINFO_CHECK}"
    rm -f "${METAINFO_CHECK}"
fi

# ---------------------------------------------------------------------------
# 5. Prepare dist/ and clean previous makepkg artifacts
# ---------------------------------------------------------------------------

mkdir -p "${DIST_DIR}"

find "${DIST_DIR}" \
    -maxdepth 1 \
    -type f \
    -name 'rustview-*.pkg.tar.*' \
    -delete

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

content="$(bsdtar -tf "${FINAL_PKG}")"
for want in usr/share/licenses/rustview/LICENSE usr/share/licenses/rustview/ATTRIBUTION.md \
    usr/share/licenses/rustview/GPL-3.0-only.txt; do
    if ! grep -qx "${want}" <<<"${content}"; then
        echo "[-] Error: the package lacks ${want}" >&2
        exit 1
    fi
done

# ---------------------------------------------------------------------------
# 10. Done
# ---------------------------------------------------------------------------

echo
echo "============================================================"
echo "[OK] RustView Arch package built successfully!"
echo "     Version: ${VERSION}"
echo "     Package: ${FINAL_PKG}"
echo "============================================================"
echo
echo "Install locally:"
echo "    sudo pacman -U \"${FINAL_PKG}\""
echo
