#!/usr/bin/env bash
# Repeatable FPS benchmark harness for the replay workflow.
#
# Runs fps_bench --replay N times against a fixed recording and prints
# mean / median / min / max / stddev so optimizations can be compared
# without squinting at single-run jitter.
#
# Usage:
#   scripts/measure.sh                      # defaults: 5 runs, current workspace
#   RUNS=10 scripts/measure.sh
#   scripts/measure.sh --label dynarec      # tag the result row
#   scripts/measure.sh --rev main           # build+measure a different git rev in a tmp worktree
set -euo pipefail

BIOS="${BIOS:-core/benches/roms/normatt_gba_bios.bin}"
ROM="${ROM:-../pokeemerald/pokeemerald.gba}"
REPLAY="${REPLAY:-/tmp/pokeemerald_run.rec}"
RUNS="${RUNS:-3}"
# The current pokeemerald_run.rec already spans ~4 min of emulated play per
# pass — one loop per sample is plenty. Bump via --loops for noise reduction.
LOOPS="${LOOPS:-1}"
FEATURES="${FEATURES:-}"
LABEL=""
REV=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --label) LABEL="$2"; shift 2 ;;
    --rev) REV="$2"; shift 2 ;;
    --runs) RUNS="$2"; shift 2 ;;
    --loops) LOOPS="$2"; shift 2 ;;
    --features) FEATURES="$2"; shift 2 ;;
    *) echo "unknown arg: $1" >&2; exit 1 ;;
  esac
done

workdir="$PWD"
if [[ -n "$REV" ]]; then
  wt="$(mktemp -d -t rba-measure-XXXX)"
  trap 'git worktree remove --force "$wt" >/dev/null 2>&1 || true; rm -rf "$wt"' EXIT
  git worktree add --quiet --detach "$wt" "$REV"
  workdir="$wt"
  [[ -z "$LABEL" ]] && LABEL="$REV"
fi
[[ -z "$LABEL" ]] && LABEL="$(git rev-parse --short HEAD)"

build_args=(--release -p fps_bench)
[[ -n "$FEATURES" ]] && build_args+=(--features "$FEATURES")

echo "[measure] building (cwd=$workdir features='$FEATURES')..." >&2
(cd "$workdir" && cargo build --quiet "${build_args[@]}") >&2

bin="$workdir/target/release/fps_bench"
[[ -x "$bin" ]] || { echo "missing binary $bin" >&2; exit 1; }

# Resolve paths from repo root so --rev worktrees still find ROM/BIOS.
abs() { case "$1" in /*) echo "$1" ;; *) echo "$PWD/$1" ;; esac; }
BIOS_ABS="$(abs "$BIOS")"
ROM_ABS="$(abs "$ROM")"
REPLAY_ABS="$(abs "$REPLAY")"

for p in "$BIOS_ABS" "$ROM_ABS" "$REPLAY_ABS"; do
  [[ -f "$p" ]] || { echo "missing: $p" >&2; exit 1; }
done

# Older builds (pre-fps_bench --loops) don't accept the flag. Detect and fall
# back so we can still bench the --rev master baseline from this script.
supports_loops=0
if "$bin" --help 2>/dev/null | grep -q -- '--loops'; then supports_loops=1; fi

samples=()
if [[ $supports_loops -eq 1 ]]; then
  echo "[measure] $LABEL: $RUNS runs x $LOOPS loops/run against $REPLAY_ABS" >&2
else
  echo "[measure] $LABEL: $RUNS runs (binary has no --loops; single pass) against $REPLAY_ABS" >&2
fi
for i in $(seq 1 "$RUNS"); do
  if [[ $supports_loops -eq 1 ]]; then
    line="$("$bin" "$BIOS_ABS" "$ROM_ABS" --replay "$REPLAY_ABS" --loops "$LOOPS" 2>/dev/null | tail -n 1)"
  else
    line="$("$bin" "$BIOS_ABS" "$ROM_ABS" --replay "$REPLAY_ABS" 2>/dev/null | tail -n 1)"
  fi
  # "replay done: N frames in T s wall, FPS avg fps (C emulated cycles)"
  fps="$(echo "$line" | sed -n 's/.* \([0-9.]*\) avg fps.*/\1/p')"
  [[ -z "$fps" ]] && { echo "no fps parsed from: $line" >&2; exit 1; }
  printf '  run %d: %s\n' "$i" "$fps" >&2
  samples+=("$fps")
done

python3 - "$LABEL" "${samples[@]}" <<'PY'
import statistics, sys
label, *vals = sys.argv[1:]
xs = [float(v) for v in vals]
mean   = statistics.mean(xs)
median = statistics.median(xs)
stdev  = statistics.stdev(xs) if len(xs) > 1 else 0.0
print(f"label={label}  mean={mean:.1f}  median={median:.1f}  "
      f"min={min(xs):.1f}  max={max(xs):.1f}  stdev={stdev:.1f}  n={len(xs)}")
PY
