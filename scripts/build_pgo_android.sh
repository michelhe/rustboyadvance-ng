#!/usr/bin/env bash
# Cross PGO for aarch64-linux-android. Two stage pipeline where the
# instrumented binary runs on the device (not host), so we collect real
# phone edge counts and feed them back into a profile use rebuild on
# host.
#
# Stages:
#   1. Instrumented build targeting aarch64-linux-android.
#   2. adb push + run against the replay N times, each run dumping a
#      .profraw into /data/local/tmp/pgo-rba/. Pull them back.
#   3. llvm-profdata merge into one .profdata.
#   4. Profile use rebuild with the merged profdata.
#
# Result: target/aarch64-linux-android/release/fps_bench optimized to
# the pokeemerald hot path on arm64, adb push + run to measure.
set -euo pipefail

BIOS="${BIOS:-core/benches/roms/normatt_gba_bios.bin}"
ROM="${ROM:-../pokeemerald/pokeemerald.gba}"
REPLAY="${REPLAY:-/tmp/pokeemerald_run.rec}"
PGO_DIR="${PGO_DIR:-$PWD/target/pgo-android}"
DEVICE_PROFDIR="${DEVICE_PROFDIR:-/data/local/tmp/pgo-rba}"
DEVICE_TMP="${DEVICE_TMP:-/data/local/tmp}"
RUNS="${RUNS:-2}"
TARGET="aarch64-linux-android"
ADB="${ADB:-/home/user/linux-platform-tools/adb}"

LLVM_PROFDATA="$(rustc --print sysroot)/lib/rustlib/x86_64-unknown-linux-gnu/bin/llvm-profdata"
if [[ ! -x "$LLVM_PROFDATA" ]]; then
  echo "missing llvm-profdata at $LLVM_PROFDATA" >&2
  echo "run: rustup component add llvm-tools-preview" >&2
  exit 1
fi
"$ADB" devices | grep -q "device$" || { echo "no device connected" >&2; exit 1; }

for p in "$BIOS" "$ROM" "$REPLAY"; do
  [[ -f "$p" ]] || { echo "missing: $p" >&2; exit 1; }
done

abs() { case "$1" in /*) echo "$1" ;; *) echo "$PWD/$1" ;; esac; }
BIOS="$(abs "$BIOS")"; ROM="$(abs "$ROM")"; REPLAY="$(abs "$REPLAY")"
mkdir -p "$PGO_DIR"
rm -f "$PGO_DIR"/*.profraw "$PGO_DIR"/merged.profdata

echo "[pgo-android] step 1: instrumented cross build" >&2
# On device the binary writes profraws to the path it was given via
# LLVM_PROFILE_FILE. Make sure the target path is writable under
# /data/local/tmp.
RUSTFLAGS="-Cprofile-generate=$DEVICE_PROFDIR" \
  cargo build --release --target "$TARGET" -p fps_bench --quiet
INSTR="target/$TARGET/release/fps_bench"

echo "[pgo-android] push instrumented binary + data" >&2
"$ADB" shell "rm -rf $DEVICE_PROFDIR && mkdir -p $DEVICE_PROFDIR"
"$ADB" push "$INSTR" "$DEVICE_TMP/fps_bench_instr" >/dev/null
"$ADB" push "$BIOS" "$DEVICE_TMP/gba_bios.bin"     >/dev/null
"$ADB" push "$ROM"  "$DEVICE_TMP/pokeemerald.gba"  >/dev/null
"$ADB" push "$REPLAY" "$DEVICE_TMP/pokeemerald_run.rec" >/dev/null
"$ADB" shell "chmod +x $DEVICE_TMP/fps_bench_instr"

echo "[pgo-android] step 2: run instrumented binary on device ($RUNS training runs)" >&2
for i in $(seq 1 "$RUNS"); do
  line="$("$ADB" shell "cd $DEVICE_TMP && LLVM_PROFILE_FILE=$DEVICE_PROFDIR/default_%m_%p.profraw ./fps_bench_instr gba_bios.bin pokeemerald.gba --replay pokeemerald_run.rec --loops 1" 2>/dev/null | tr -d '\r' | tail -n 1)"
  echo "  training run $i: $line" >&2
done

echo "[pgo-android] step 3: pull profraws + merge" >&2
"$ADB" pull "$DEVICE_PROFDIR" "$PGO_DIR.pulled" >/dev/null
# Move any profraws flat into PGO_DIR.
find "$PGO_DIR.pulled" -name '*.profraw' -exec cp -f {} "$PGO_DIR"/ \;
rm -rf "$PGO_DIR.pulled"
ls "$PGO_DIR"/*.profraw 2>/dev/null | head -5
"$LLVM_PROFDATA" merge -o "$PGO_DIR/merged.profdata" "$PGO_DIR"/*.profraw

echo "[pgo-android] step 4: profile use rebuild" >&2
cargo clean --target "$TARGET" -p fps_bench -p rustboyadvance-core -p arm7tdmi --quiet 2>/dev/null || true
RUSTFLAGS="-Cprofile-use=$PGO_DIR/merged.profdata" \
  cargo build --release --target "$TARGET" -p fps_bench --quiet

echo "[pgo-android] push + smoke run PGO binary" >&2
"$ADB" push "$INSTR" "$DEVICE_TMP/fps_bench_pgo" >/dev/null
"$ADB" shell "chmod +x $DEVICE_TMP/fps_bench_pgo"
"$ADB" shell "cd $DEVICE_TMP && ./fps_bench_pgo gba_bios.bin pokeemerald.gba --replay pokeemerald_run.rec --loops 1" 2>/dev/null | tr -d '\r' | tail -n 2

echo "[pgo-android] done. binary on device at $DEVICE_TMP/fps_bench_pgo" >&2
