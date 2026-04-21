#!/usr/bin/env bash
# Two step PGO build of fps_bench against the recorded pokeemerald session.
#
# Step 1: build an instrumented binary that writes .profraw files into $PGO_DIR.
# Step 2: run the instrumented binary against the replay to collect edge counts.
# Step 3: merge the .profraw files into a single .profdata with llvm-profdata.
# Step 4: rebuild with -Cprofile-use pointing at that .profdata.
#
# Result lands at target/x86_64-unknown-linux-gnu/release/fps_bench and is
# typically 30 to 40 percent faster than a plain `cargo build --release`
# fps_bench on pokeemerald because LLVM gets the real hot edges and cold
# edges right (branch layout, inlining, block reorder).
#
# Usage:
#   scripts/build_pgo.sh                # uses defaults below
#   RUNS=3 scripts/build_pgo.sh         # bump training runs
set -euo pipefail

BIOS="${BIOS:-core/benches/roms/normatt_gba_bios.bin}"
ROM="${ROM:-../pokeemerald/pokeemerald.gba}"
REPLAY="${REPLAY:-/tmp/pokeemerald_run.rec}"
PGO_DIR="${PGO_DIR:-$PWD/target/pgo-data}"
RUNS="${RUNS:-2}"
TARGET="${TARGET:-x86_64-unknown-linux-gnu}"

LLVM_PROFDATA="$(rustc --print sysroot)/lib/rustlib/${TARGET}/bin/llvm-profdata"
if [[ ! -x "$LLVM_PROFDATA" ]]; then
  echo "missing llvm-profdata at $LLVM_PROFDATA" >&2
  echo "run: rustup component add llvm-tools-preview" >&2
  exit 1
fi

for p in "$BIOS" "$ROM" "$REPLAY"; do
  [[ -f "$p" ]] || { echo "missing: $p" >&2; exit 1; }
done

abs() { case "$1" in /*) echo "$1" ;; *) echo "$PWD/$1" ;; esac; }
BIOS="$(abs "$BIOS")"
ROM="$(abs "$ROM")"
REPLAY="$(abs "$REPLAY")"
mkdir -p "$PGO_DIR"
rm -f "$PGO_DIR"/*.profraw "$PGO_DIR"/merged.profdata

echo "[pgo] step 1: instrumented build" >&2
RUSTFLAGS="-Cprofile-generate=$PGO_DIR" \
  cargo build --release --target "$TARGET" -p fps_bench --quiet

INSTR_BIN="target/${TARGET}/release/fps_bench"

echo "[pgo] step 2: collect profiles ($RUNS runs)" >&2
for i in $(seq 1 "$RUNS"); do
  line="$("$INSTR_BIN" "$BIOS" "$ROM" --replay "$REPLAY" --loops 1 2>/dev/null | tail -n 1)"
  echo "  training run $i: $line" >&2
done

echo "[pgo] step 3: merge profdata" >&2
"$LLVM_PROFDATA" merge -o "$PGO_DIR/merged.profdata" "$PGO_DIR"/*.profraw

echo "[pgo] step 4: profile use rebuild" >&2
# Clean the instrumented artifacts so the profile-use build actually re-emits
# codegen with the merged profile.
cargo clean --target "$TARGET" -p fps_bench -p rustboyadvance-core -p arm7tdmi --quiet 2>/dev/null || true
RUSTFLAGS="-Cprofile-use=$PGO_DIR/merged.profdata" \
  cargo build --release --target "$TARGET" -p fps_bench --quiet

echo "[pgo] done. binary: $INSTR_BIN" >&2
"$INSTR_BIN" --help >/dev/null 2>&1 || true
