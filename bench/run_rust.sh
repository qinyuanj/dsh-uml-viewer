#!/bin/sh
# Rust viewer run of the benchmark protocol 1 (bench/README.md, #143).
# Usage: bench/run_rust.sh <synthetic-N.edn> <out-dir>
# Needs a WSLg desktop session and `cargo build --release --locked` first.
# Optional: UML_VIEWER_BENCH_ONLY=<phase> (one phase), BENCH_TIMEOUT seconds
# (default 600), UML_BENCH_MAX_LOAD (default 2.0: a 1-min load above it at the
# start or the end voids the run), UML_BENCH_WAIT_LOAD_SECS (default 0: wait
# up to N s for the load to drop to the maximum before starting).
# Exit 0 = every protocol-1 criterion passed on a D3D12 renderer under the
# load maximum; 1 = a criterion failed (verdict=FAIL) or the run is void
# (verdict=VOID, with `void reason=` lines; see summary.txt).
set -eu
REPO=$(cd "$(dirname "$0")/.." && pwd)
IR=$(realpath "$1"); OUT=$2
BINARY=${UML_VIEWER_RUST_EXECUTABLE:-$REPO/target/release/uml-viewer}
if [ ! -x "$BINARY" ]; then
  echo "missing $BINARY; run cargo build --release --locked" >&2
  exit 2
fi
MAX_LOAD=${UML_BENCH_MAX_LOAD:-2.0}
WAIT=${UML_BENCH_WAIT_LOAD_SECS:-0}
if ! awk -v m="$MAX_LOAD" 'BEGIN { exit !(m ~ /^[0-9]*\.?[0-9]+$/ && m > 0) }'; then
  echo "UML_BENCH_MAX_LOAD must be a positive number, got '$MAX_LOAD'" >&2
  exit 2
fi
case $WAIT in ''|*[!0-9]*)
  echo "UML_BENCH_WAIT_LOAD_SECS must be whole seconds, got '$WAIT'" >&2
  exit 2;;
esac
# Provenance before the output directory exists, so a results dir inside the
# repository cannot make the tree look dirty.
COMMIT=$(git -C "$REPO" rev-parse HEAD 2>/dev/null || true)
if [ -z "$COMMIT" ]; then DIRTY=
elif [ -n "$(git -C "$REPO" status --porcelain 2>/dev/null)" ]; then DIRTY=true
else DIRTY=false
fi
load1() { cut -d' ' -f1 /proc/loadavg; }
above() { awk -v l="$(load1)" -v m="$MAX_LOAD" 'BEGIN { exit !(l > m) }'; }
waited=0
while [ "$waited" -lt "$WAIT" ] && above; do
  step=$((WAIT - waited)); [ "$step" -gt 5 ] && step=5
  sleep "$step"; waited=$((waited + step))
done
mkdir -p "$OUT"; OUT=$(realpath "$OUT")
# An isolated project root: no .metrics overlay, no mailbox, no companion.
PROJECT=$(mktemp -d); cp "$IR" "$PROJECT/diagram.edn"
status=0
GALLIUM_DRIVER=d3d12 WGPU_BACKEND=gl \
  UML_BENCH_COMMIT="$COMMIT" UML_BENCH_DIRTY="$DIRTY" \
  UML_BENCH_MAX_LOAD="$MAX_LOAD" UML_BENCH_LOAD_WAITED_S="$waited" \
  UML_VIEWER_BENCH="$OUT" UML_VIEWER_EVIDENCE="$OUT" UML_VIEWER_PROJECT_ROOT="$PROJECT" \
  timeout "${BENCH_TIMEOUT:-600}" "$BINARY" viewer "$PROJECT/diagram.edn" \
  >"$OUT/stdout.txt" 2>"$OUT/stderr.txt" || status=$?
rm -rf "$PROJECT"
echo "exit=$status"; cat "$OUT/summary.txt" 2>/dev/null || tail -5 "$OUT/stderr.txt"
exit $status
