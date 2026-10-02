#!/usr/bin/env bash
# Release notes for VERSION: its CHANGELOG.md section, then install steps and
# facts about the built binary. Used by release.yml and release-notes.yml.
#
# usage: release-notes.sh VERSION BINARY ARCHIVE COMMIT > notes.md
set -euo pipefail

version=$1 bin=$2 archive=$3 commit=$4
repo=${GITHUB_REPOSITORY:-jerichosiahaya/snip}
crate=$(sed -n 's/^name = "\(.*\)"$/\1/p' Cargo.toml | head -n1)
asset=snip-arch-linux-x86_64.tar.gz

mb() { awk -v b="$1" 'BEGIN { printf "%.2f MB", b / 1048576 }'; }
glibc=$(objdump -T "$bin" | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -uV | tail -n1)

awk -v v="$version" '/^## /{p = index($0, "[" v "]") > 0; next} p' CHANGELOG.md
echo "## Install (Arch Linux x86-64)"
echo
echo '```bash'
echo "curl -fLO https://github.com/$repo/releases/download/v$version/$asset"
echo "curl -fLO https://github.com/$repo/releases/download/v$version/SHA256SUMS"
echo "sha256sum -c SHA256SUMS && tar -xzf $asset && install -Dm755 snip \"\$HOME/.local/bin/snip\""
echo '```'
echo
echo "- Executable: $(mb "$(stat -c %s "$bin")"); download archive: $(mb "$(stat -c %s "$archive")")."
echo "- Needs glibc $glibc or newer and libgcc. SQLite is bundled; no Rust needed."
echo "- Built on archlinux:latest from commit $commit. Not an ARM build."
# only point at crates.io once the crate is actually there
if curl -fsS -A "$repo release notes" "https://crates.io/api/v1/crates/$crate" >/dev/null 2>&1; then
  echo "- Or build from source on any platform: \`cargo install $crate\`."
else
  echo "- Or build from source: \`git clone https://github.com/$repo && cd ${repo#*/} && cargo build --release --locked\`."
fi
