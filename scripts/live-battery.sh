#!/usr/bin/env bash
# Live test battery for ndac against a throwaway NextDNS profile.
# Usage: KEY_FILE=path/to/key PROFILE=abc123 scripts/live-battery.sh
# KEY_FILE holds only the API key (chmod 600). Restores the profile at the
# end. Needs both builds: cargo build -p ndac, and
# cargo build -p ndac --features unstable-endpoints --target-dir target/unstable
set -uo pipefail

KEY_FILE=${KEY_FILE:?set KEY_FILE to a file holding the API key}
REPO=${REPO:-$(git -C "$(dirname "$0")" rev-parse --show-toplevel)}
PROFILE=${PROFILE:?set PROFILE to the id of a throwaway profile}
W=$(mktemp -d)
trap 'rm -rf "$W"' EXIT
PASS=0
FAIL=0


run_with() {
  local bin=$1 dir=$2; shift 2
  (cd "$dir" && NEXTDNS_API_KEY=$(cat "$KEY_FILE") XDG_CONFIG_HOME="$W/.xdg" \
    NDAC_AUTH_SOURCE=env:NEXTDNS_API_KEY NO_COLOR=1 "$bin" "$@")
}
n() { local dir=$1; shift; run_with "$REPO/target/debug/ndac" "$dir" "$@"; }
u() { local dir=$1; shift; run_with "$REPO/target/unstable/debug/ndac" "$dir" "$@"; }

# expect CODE NAME CMD...: runs CMD, keeps output in $OUT, checks the exit code.
expect() {
  local want=$1 name=$2; shift 2
  OUT=$("$@" 2>&1); local got=$?
  if [ "$got" = "$want" ]; then PASS=$((PASS + 1)); printf 'ok    %s\n' "$name"
  else FAIL=$((FAIL + 1)); printf 'FAIL  %s (exit %s, want %s)\n%s\n' "$name" "$got" "$want" "$(printf '%s' "$OUT" | tail -8 | sed 's/^/      /')"; fi
}
# has NAME PATTERN: checks that the last $OUT contains PATTERN.
has() {
  if printf '%s' "$OUT" | grep -q -- "$2"; then PASS=$((PASS + 1)); printf 'ok    %s\n' "$1"
  else FAIL=$((FAIL + 1)); printf 'FAIL  %s (missing %q)\n' "$1" "$2"; fi
}
api() {
  curl -sS -o /dev/null -w '%{http_code}' -X "$1" -H "X-Api-Key: $(cat "$KEY_FILE")" \
    -H 'Content-Type: application/json' "https://api.nextdns.io/profiles/$PROFILE$2" ${3:+-d "$3"}
}
edit() { python3 - "$1" "$2" "$3" <<'EOF'
import sys
p, old, new = sys.argv[1:]
t = open(p).read()
assert old in t, f"{old!r} not in {p}"
open(p, "w").write(t.replace(old, new, 1))
EOF
}

A=$W/main
B=$W/fresh
mkdir -p "$A" "$B"
F=$A/profiles/perfil.toml

echo "== setup and read-only"
expect 0 "init" n "$A" init
expect 0 "auth check" n "$A" auth check; has "auth check reaches the profile" "$PROFILE"
expect 0 "auth sources" n "$A" auth sources; has "auth sources uses env" "used"
expect 0 "import --profile" n "$A" import --profile "$PROFILE"
cp "$F" "$W/orig.toml"
expect 0 "validate" n "$A" validate
expect 0 "fmt --check" n "$A" fmt --check
expect 0 "render toml" n "$A" render perfil
expect 0 "render json" n "$A" render perfil --format json; has "render json has id" "\"id\": \"$PROFILE\""
expect 0 "plan after import" n "$A" plan --detailed-exitcode
expect 0 "apply -q with nothing to do succeeds" n "$A" -q apply --auto-approve
expect 0 "plan json" n "$A" plan --format json; has "plan json summary" '"summary"'
expect 0 "plan markdown" n "$A" plan --format markdown; has "plan markdown header" "## ndac plan"
expect 0 "drift after import" n "$A" drift
expect 0 "snapshot list" n "$A" snapshot list

echo "== error cases"
expect 4 "plan rejects a target matching no profile" n "$A" plan --target nope
cp "$F" "$W/f.bak"; edit "$F" "web3 = false" "web3 = 1"
expect 4 "validate rejects a wrong type" n "$A" validate
cp "$W/f.bak" "$F"
expect 1 "unknown argument (usage errors exit 1)" n "$A" plan --nope
(cd "$A" && NEXTDNS_API_KEY=0000000000000000000000000000000000000000 XDG_CONFIG_HOME="$W/.xdg" \
  NDAC_AUTH_SOURCE=env:NEXTDNS_API_KEY NO_COLOR=1 "$REPO/target/debug/ndac" auth check >"$W/badkey.txt" 2>&1)
code=$?; OUT=$(cat "$W/badkey.txt")
if [ "$code" != 0 ]; then PASS=$((PASS + 1)); echo "ok    auth check rejects a bad key (exit $code)"
else FAIL=$((FAIL + 1)); echo "FAIL  auth check accepted a bad key"; fi
expect 0 "import of an unknown id reports it" n "$B" init
OUT=$(n "$B" import --profile zzzzzz 2>&1); code=$?
if [ "$code" != 0 ]; then PASS=$((PASS + 1)); echo "ok    import rejects an unknown id (exit $code)"
else FAIL=$((FAIL + 1)); echo "FAIL  import accepted an unknown id"; fi

echo "== apply across every section"
edit "$F" "nrd                     = false" "nrd                     = true"
edit "$F" "tlds                    = []" 'tlds                    = ["mov", "zip"]'
edit "$F" 'blocklists        = ["nextdns-recommended"]' 'blocklists        = ["nextdns-recommended", "oisd"]'
edit "$F" "natives           = []" 'natives           = ["samsung"]'
edit "$F" "safeSearch            = false" "safeSearch            = true"
edit "$F" $'[parentalControl.services]\n' $'[parentalControl.services]\ntiktok = true\n'
edit "$F" $'[parentalControl.categories]\n' $'[parentalControl.categories]\n"social-networks" = true\n'
edit "$F" $'[denylist.items]\n' $'[denylist.items]\n"ndac-test-deny.example" = true\n"ndac-test-off.example" = false\n'
edit "$F" $'[allowlist.items]\n' $'[allowlist.items]\n"ndac-test-allow.example" = true\n'
edit "$F" 'retention = "90d"' 'retention = "30d"'
edit "$F" $'[settings.blockPage]\nenabled = false' $'[settings.blockPage]\nenabled = true'
edit "$F" "cacheBoost      = false" "cacheBoost      = true"
expect 0 "fmt" n "$A" fmt
expect 0 "fmt --check after fmt" n "$A" fmt --check
expect 2 "plan shows changes" n "$A" plan --detailed-exitcode; has "plan counts" "9 to add, 5 to change"
expect 0 "plan --target denylist only" n "$A" plan --target perfil.denylist; has "target limits plan" "ndac-test-deny.example"
expect 0 "apply" n "$A" apply --auto-approve; has "apply verified" "applied and verified"
expect 0 "plan empty after apply" n "$A" plan --detailed-exitcode
expect 0 "drift empty after apply" n "$A" drift

echo "== change, remove, destroy guard"
edit "$F" '"ndac-test-deny.example" = true' '"ndac-test-deny.example" = false'
sed -i '/"ndac-test-off.example"/d' "$F"
sed -i 's/tlds *= \["mov", "zip"\]/tlds = ["zip"]/' "$F"
n "$A" fmt -q >/dev/null 2>&1
expect 2 "plan shows update and removals" n "$A" plan --detailed-exitcode; has "plan shows a removal" "will be removed"
expect 0 "apply update and removals" n "$A" apply --auto-approve
expect 0 "plan empty after removals" n "$A" plan --detailed-exitcode
sed -i '/"ndac-test-allow.example"/d' "$F"
expect 1 "apply refuses to empty a list" n "$A" apply --auto-approve; has "destroy guard message" "remove every item of"
expect 0 "apply --allow-destroy empties it" n "$A" apply --auto-approve --allow-destroy
expect 0 "plan empty after destroy" n "$A" plan --detailed-exitcode

echo "== drift made outside ndac"
c1=$(api PATCH /settings/performance '{"cacheBoost":false}')
c2=$(api POST /denylist '{"id":"drift-added.example","active":true}')
c3=$(api DELETE /privacy/blocklists/oisd)
c4=$(api PATCH /security '{"nrd":false}')
echo "      API calls: $c1 $c2 $c3 $c4"
expect 2 "drift detects outside changes" n "$A" drift; has "drift in 4 sections" "Drift in 4 sections"
expect 2 "drift github" n "$A" drift --format github; has "github annotations" "::warning file=profiles/perfil.toml"
expect 2 "drift json" n "$A" drift --format json; has "drift json" '"drift_sections": 4'
mkdir -p "$W/prom"
expect 2 "drift prom textfile" n "$A" drift --format prom --textfile-dir "$W/prom"
OUT=$(cat "$W/prom/ndac_drift.prom"); has "prom file has drift" 'ndac_drift{profile="perfil"'
expect 2 "plan also shows drift" n "$A" plan --detailed-exitcode; has "plan drift tag" "[drift]"

echo "== reconcile --prefer live (fix #4: file stays formatted)"
expect 0 "reconcile --prefer live" n "$A" reconcile --prefer live --non-interactive; has "4 imported" "4 imported"
OUT=$(cat "$F"); has "imported comment" "# imported "
expect 0 "fmt --check after reconcile" n "$A" fmt --check
expect 0 "validate after reconcile" n "$A" validate
expect 0 "plan empty after reconcile" n "$A" plan --detailed-exitcode
expect 0 "drift empty after reconcile" n "$A" drift

echo "== reconcile --prefer code (fix #5: --allow-destroy)"
c1=$(api PATCH /parentalControl '{"safeSearch":false}')
c2=$(api POST /allowlist '{"id":"drift-allow.example","active":true}')
echo "      API calls: $c1 $c2"
expect 2 "drift before prefer code" n "$A" drift
expect 1 "reconcile code refuses to empty a list" n "$A" reconcile --prefer code --non-interactive; has "guard message" "remove every item of"
expect 2 "nothing changed after refusal" n "$A" drift
expect 0 "reconcile code --allow-destroy" n "$A" reconcile --prefer code --non-interactive --allow-destroy; has "overwrite verified" "applied and verified"
expect 0 "plan empty after prefer code" n "$A" plan --detailed-exitcode
expect 0 "drift empty after prefer code" n "$A" drift

echo "== snapshots and rollback"
expect 0 "snapshot list" n "$A" snapshot list
PRE=$(n "$A" snapshot list 2>/dev/null | awk '$3=="pre-apply"{print $1; exit}')
LAST=$(n "$A" snapshot list 2>/dev/null | tail -1 | awk '{print $1}')
expect 0 "snapshot show" n "$A" snapshot show "$PRE"; has "show kind" '"kind": "pre-apply"'
expect 0 "snapshot diff" n "$A" snapshot diff "$PRE" "$LAST"; has "diff has changes" "to add"
expect 0 "snapshot show by prefix" n "$A" snapshot show "${PRE:0:20}"
expect 0 "rollback to first pre-apply" n "$A" rollback "$PRE" --auto-approve --allow-destroy; has "rollback verified" "applied and verified"
cp "$W/orig.toml" "$F"
expect 0 "live equals the original after rollback" n "$A" plan --detailed-exitcode

echo "== endpoint, doctor and helpers"
for t in android doh-url networkmanager nextdns-cli resolved; do
  expect 0 "endpoint $t" n "$A" endpoint render perfil --target "$t"
done
n "$A" endpoint render perfil --target networkmanager >"$W/nm.sh" 2>/dev/null
expect 0 "networkmanager script passes shellcheck" shellcheck "$W/nm.sh"
OUT=$(n "$A" doctor --expect-profile perfil 2>&1); code=$?
if [ "$code" = 0 ] || [ "$code" = 2 ]; then PASS=$((PASS + 1)); echo "ok    doctor runs (exit $code: $(printf '%s' "$OUT" | head -1))"
else FAIL=$((FAIL + 1)); echo "FAIL  doctor exit $code"; fi
for k in root local profile user; do
  OUT=$(n "$A" schema --kind "$k" 2>&1)
  if printf '%s' "$OUT" | python3 -c 'import json,sys; json.load(sys.stdin)' 2>/dev/null; then PASS=$((PASS + 1)); echo "ok    schema $k is JSON"
  else FAIL=$((FAIL + 1)); echo "FAIL  schema $k"; fi
done
for sh in bash zsh fish elvish powershell; do expect 0 "completions $sh" n "$A" completions "$sh"; done
expect 0 "man" n "$A" man; has "man header" ".TH ndac 1"
expect 0 "--version" n "$A" --version; has "version" "ndac $(sed -n 's/^version = "\(.*\)"/\1/p' "$REPO/Cargo.toml" | head -1)"
expect 0 "import --all" n "$B" import --all
expect 0 "re-import skips" n "$B" import --profile "$PROFILE"; has "skip message" "already exists"
expect 0 "import --force" n "$B" import --profile "$PROFILE" --force
if cmp -s "$B/profiles/perfil.toml" "$W/orig.toml"; then PASS=$((PASS + 1)); echo "ok    re-import identical to the original"
else FAIL=$((FAIL + 1)); echo "FAIL  re-import differs"; diff "$B/profiles/perfil.toml" "$W/orig.toml" | head; fi

echo "== unstable-endpoints build: rewrites and IPv6"
C=$W/unstable; mkdir -p "$C"
expect 0 "unstable init" u "$C" init
expect 0 "unstable import" u "$C" import --profile "$PROFILE"
G=$C/profiles/perfil.toml; cp "$G" "$W/uorig.toml"
edit "$G" $'[rewrites.items]\n' $'[rewrites.items]\n"router.ndac-test.lan" = "192.168.1.1"\n"nas.ndac-test.lan" = "nas.example"\n"v6.ndac-test.lan" = "fd00::1"\n'
u "$C" fmt -q >/dev/null 2>&1
expect 2 "rewrites plan" u "$C" plan --detailed-exitcode; has "3 rewrites" "3 to add"
expect 0 "rewrites apply" u "$C" apply --auto-approve
expect 0 "rewrites plan empty" u "$C" plan --detailed-exitcode
edit "$G" '"192.168.1.1"' '"192.168.1.2"'
expect 0 "rewrite answer change" u "$C" apply --auto-approve
expect 0 "plan empty after answer change" u "$C" plan --detailed-exitcode
c1=$(api POST /rewrites '{"name":"drift.ndac-test.lan","content":"10.0.0.9"}'); echo "      API call: $c1"
expect 2 "rewrite drift" u "$C" drift; has "rewrite drift item" "drift.ndac-test.lan"
for t in resolved networkmanager; do
  expect 0 "endpoint $t --api-ipv6" u "$C" endpoint render perfil --target "$t" --api-ipv6; has "$t has IPv6" "2a07:"
done
cp "$W/uorig.toml" "$G"
expect 0 "rewrites removed" u "$C" apply --auto-approve --allow-destroy
expect 0 "unstable plan empty" u "$C" plan --detailed-exitcode
expect 0 "unstable drift empty" u "$C" drift

echo "== final state"
expect 0 "fresh import" n "$B" import --profile "$PROFILE" --force
if cmp -s "$B/profiles/perfil.toml" "$W/orig.toml"; then PASS=$((PASS + 1)); echo "ok    profile restored to the original"
else FAIL=$((FAIL + 1)); echo "FAIL  profile differs from the original"; diff "$B/profiles/perfil.toml" "$W/orig.toml"; fi
expect 0 "unstable fresh import" u "$C" import --profile "$PROFILE" --force
if cmp -s "$G" "$W/uorig.toml"; then PASS=$((PASS + 1)); echo "ok    rewrites restored (empty)"
else FAIL=$((FAIL + 1)); echo "FAIL  rewrites differ"; fi

echo
echo "passed=$PASS failed=$FAIL"
[ "$FAIL" = 0 ]
