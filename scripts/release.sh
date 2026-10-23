#!/usr/bin/env bash
# Releases of RustView always come with a changelog: the notes of a GitHub release are the section of CHANGELOG.md for
# that version (Keep a Changelog format). Assets: the package, the same package as rustview-x86_64.pkg.tar.zst (so that
# releases/latest/download/rustview-x86_64.pkg.tar.zst is a permanent link to the newest one) and SHA256SUMS.
#
#   scripts/release.sh --check        the changelog documents the version in the root Cargo.toml (used by scripts/check-all.sh)
#   scripts/release.sh --notes-only   print the release notes of that version (for pasting into the GitHub web form)
#   scripts/release.sh                prepare the release in dist/ (checksum, notes) and print the commands; changes nothing
#   scripts/release.sh --publish      also create and push the tag v<version> and create the GitHub release
#                                     (needs `gh auth login`; the repository is private, so the release is private too)
#
# "finalisiere" runs --publish on master after the merge: clean tree, CHANGELOG section dated, package built by check-all.
set -euo pipefail
cd "$(dirname "$0")/.."

mode=prepare
for arg in "$@"; do
  case "$arg" in
    --check) mode=check ;;
    --notes-only) mode=notes ;;
    --publish) mode=publish ;;
    *) echo "unknown option: $arg" >&2; exit 2 ;;
  esac
done

fail() { echo "release: $*" >&2; exit 1; }

version=$(sed -n '/^\[workspace.package\]/,/^\[/{s/^version *= *"\(.*\)"/\1/p}' Cargo.toml)
[ -n "$version" ] || fail "cannot read the version from Cargo.toml"
tag="v$version"
pkg_name="rustview-${version}-1-x86_64.pkg.tar.zst"
pkg="dist/$pkg_name"
# The same package under a name without the version: GitHub's .../releases/latest/download/<name> link then always serves
# the newest release (a versioned name changes with every release).
alias_name="rustview-x86_64.pkg.tar.zst"
alias_file="dist/$alias_name"
sums="dist/SHA256SUMS"
repo_url="https://github.com/vommie/rustview"

# The body of a changelog section ("## [x.y.z] - date" or "## [Unreleased]") up to the next section.
section() {
  awk -v want="$1" '
    /^## \[/ { if (found) exit; if (index($0, "## [" want "]") == 1) { found = 1; next } }
    found { print }
    /^\[[^]]+\]: / { if (found) exit }
  ' CHANGELOG.md
}
has_text() { [ -n "$(printf '%s' "$1" | tr -d '[:space:]')" ]; }

if [ "$mode" = check ]; then
  [ -f CHANGELOG.md ] || fail "CHANGELOG.md is missing"
  pkgver=$(sed -n 's/^pkgver=//p' packaging/PKGBUILD)
  [ "$pkgver" = "$version" ] || fail "packaging/PKGBUILD has pkgver=$pkgver but Cargo.toml says $version (bump both in the release commit)"
  if grep -q "^## \[$version\]" CHANGELOG.md; then
    has_text "$(section "$version")" || fail "the changelog section of $version is empty"
  elif has_text "$(section Unreleased)"; then
    :  # the next version is being collected under [Unreleased]
  else
    fail "CHANGELOG.md documents neither $version nor [Unreleased]: add an entry for what changed"
  fi
  echo "changelog ok ($version)"
  exit 0
fi

header=$(grep -m1 "^## \[$version\]" CHANGELOG.md || true)
[ -n "$header" ] || fail "CHANGELOG.md has no section for $version (rename [Unreleased] when finalizing)"
printf '%s' "$header" | grep -Eq ' - [0-9]{4}-[0-9]{2}-[0-9]{2}$' || fail "the changelog section of $version has no date"
notes=$(section "$version")
has_text "$notes" || fail "the changelog section of $version is empty"

make_notes() {
  printf '%s\n' "$notes" | sed '/./,$!d'
  cat <<EOF

## Install (Arch Linux, Manjaro)

\`\`\`bash
gh release download ${tag} --repo vommie/rustview --pattern '${pkg_name}' --pattern SHA256SUMS
sha256sum -c SHA256SUMS --ignore-missing
sudo pacman -U ${pkg_name}
\`\`\`

The newest version is always the latest release: \`gh release download --repo vommie/rustview --pattern 'rustview-x86_64.pkg.tar.zst' --pattern SHA256SUMS\`
(GitHub CLI, \`gh auth login\` once; this also works while the repository is private). Once the repository is public the permanent link
\`${repo_url}/releases/latest/download/${alias_name}\` works with a plain \`curl -LO\`.

Full changelog: ${repo_url}/blob/${tag}/CHANGELOG.md
EOF
}

if [ "$mode" = notes ]; then
  make_notes
  exit 0
fi

# --- prepare / publish --------------------------------------------------------------------------------------------------
git diff --quiet && git diff --cached --quiet || fail "the working tree has uncommitted changes"
branch=$(git rev-parse --abbrev-ref HEAD)
[ "$branch" = master ] || fail "release from master (now on $branch): finalize first"
if git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
  [ "$(git rev-parse "$tag^{commit}")" = "$(git rev-parse HEAD)" ] || fail "the tag $tag exists but does not point at HEAD"
fi

[ -f "$pkg" ] || ./build-package.sh
[ -f "$pkg" ] || fail "no package at $pkg"
cp -f "$pkg" "$alias_file"
(cd dist && sha256sum "$pkg_name" "$alias_name" > SHA256SUMS)
notes_file="dist/release-notes-${version}.md"
make_notes > "$notes_file"
echo "Prepared: $pkg, $alias_file, $sums, $notes_file"

if [ "$mode" = prepare ]; then
  cat <<EOF

Nothing was published. To publish (public; the maintainer decides):
  git tag -a $tag -m "RustView $version"     (unless it exists)
  git push origin master $tag
  gh release create $tag $pkg $alias_file $sums --title "RustView $version" --notes-file $notes_file --verify-tag --latest
or use the GitHub web form (Releases > Draft a new release, tag $tag, paste $notes_file, attach the three files).
EOF
  exit 0
fi

command -v gh >/dev/null || fail "gh is not installed (sudo pacman -S github-cli)"
gh auth status >/dev/null 2>&1 || fail "gh is not logged in: run \`gh auth login\` first"
git rev-parse -q --verify "refs/tags/$tag" >/dev/null || git tag -a "$tag" -m "RustView $version"
git push origin master "$tag"
gh release create "$tag" "$pkg" "$alias_file" "$sums" --title "RustView $version" --notes-file "$notes_file" --verify-tag --latest
echo "Published: $(gh release view "$tag" --json url -q .url)"
echo "Newest package, permanent link: ${repo_url}/releases/latest/download/${alias_name}"
