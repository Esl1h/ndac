#!/bin/sh
# Builds the release archive for one target:
#   OUT_DIR/ndac-VERSION-TARGET.tar.gz
# containing the binary, licenses, README, CHANGELOG, completions, the man
# page and, for Linux targets, the systemd units.
# Usage: archive.sh VERSION TARGET NDAC_BINARY ASSETS_DIR OUT_DIR
set -eu

version=$1
target=$2
bin=$3
assets=$4
out=$5

root=$(cd "$(dirname "$0")/../.." && pwd)
name="ndac-$version-$target"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

mkdir -p "$work/$name" "$out"
cp "$bin" "$work/$name/ndac"
chmod 0755 "$work/$name/ndac"
cp "$root/LICENSE-MIT" "$root/LICENSE-APACHE" "$root/README.md" \
  "$root/CHANGELOG.md" "$work/$name/"
cp -R "$assets/completions" "$assets/man" "$work/$name/"
case "$target" in
  *-linux-*) cp -R "$root/packaging/systemd" "$work/$name/" ;;
esac

# Reproducible archive: fixed owner, sorted entries, mtime of the commit.
mtime=$(git -C "$root" log -1 --format=%ct 2>/dev/null || echo 0)
tar --sort=name --owner=0 --group=0 --numeric-owner --mtime="@$mtime" \
  -C "$work" -cf - "$name" | gzip -9n >"$out/$name.tar.gz"
echo "$out/$name.tar.gz"
