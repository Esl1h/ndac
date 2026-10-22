#!/bin/sh
# Generates shell completions and the man page with a built ndac binary.
# Usage: assets.sh NDAC_BINARY OUT_DIR
set -eu

bin=$1
out=$2

mkdir -p "$out/completions" "$out/man"
"$bin" completions bash >"$out/completions/ndac.bash"
"$bin" completions zsh >"$out/completions/_ndac"
"$bin" completions fish >"$out/completions/ndac.fish"
"$bin" completions elvish >"$out/completions/ndac.elv"
"$bin" completions powershell >"$out/completions/_ndac.ps1"
"$bin" man >"$out/man/ndac.1"
