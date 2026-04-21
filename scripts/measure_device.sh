#!/usr/bin/env bash
# On-device FPS measurement via logcat.
#
# Assumes the Android app is already installed, the ROM has been imported,
# and the user has navigated into gameplay. Clears logcat, waits SECONDS
# seconds while the JNI emulator main loop prints one "FPS N" per second
# (target "RustdroidFps"), then summarizes the samples.
#
# Usage:
#   scripts/measure_device.sh                      # 30-second sample
#   SECONDS=60 scripts/measure_device.sh --label dynarec-device
set -euo pipefail

ADB="${ADB:-/home/user/linux-platform-tools/adb}"
PKG="${PKG:-com.mrmichel.rustdroid_emu}"
SECONDS_TO_SAMPLE="${SECONDS_TO_SAMPLE:-30}"
LABEL=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --label) LABEL="$2"; shift 2 ;;
    --seconds) SECONDS_TO_SAMPLE="$2"; shift 2 ;;
    *) echo "unknown arg: $1" >&2; exit 1 ;;
  esac
done
[[ -z "$LABEL" ]] && LABEL="device-$(git rev-parse --short HEAD 2>/dev/null || echo unknown)"

"$ADB" devices | grep -q "device$" || { echo "no device connected" >&2; exit 1; }

# Make sure the app is actually in foreground — if it isn't, we'll sample
# FPS lines from an earlier backgrounded session and get garbage.
focus="$("$ADB" shell dumpsys activity activities 2>/dev/null | grep -E 'ResumedActivity|mResumedActivity' | head -1 || true)"
if ! echo "$focus" | grep -q "$PKG"; then
  echo "[warn] $PKG not in foreground (focus=$focus). Bring the emulator to front and retry." >&2
  exit 1
fi

echo "[measure-device] $LABEL: sampling $SECONDS_TO_SAMPLE seconds..." >&2
"$ADB" logcat -c
sleep "$SECONDS_TO_SAMPLE"
# The FPS lines come out under tag "EmulatorBindings" rather than the
# RustdroidFps target the rust-side info! call requests, because the
# android_log backend rewrites the tag to the crate name. Grep anything
# with "FPS N" to be resilient to that.
samples_raw="$("$ADB" logcat -d -v brief 2>/dev/null | grep 'FPS [0-9]')"

# Lines look like: "I/EmulatorBindings(1234): <unknown>: FPS 923"
fps_list="$(echo "$samples_raw" | sed -n 's/.*FPS \([0-9]\+\).*/\1/p')"
if [[ -z "$fps_list" ]]; then
  echo "[measure-device] no FPS lines captured — is the JNI build logging? (target=RustdroidFps)" >&2
  echo "[measure-device] raw sample of recent logs follows:" >&2
  echo "$samples_raw" | tail -20 >&2
  exit 1
fi
count=$(echo "$fps_list" | wc -l)
echo "[measure-device] captured $count FPS samples" >&2

python3 - "$LABEL" <<PY
import statistics, sys
label = sys.argv[1]
xs = [float(v) for v in """$fps_list""".split() if v]
if not xs:
    print(f"label={label}  NO_SAMPLES")
    sys.exit(1)
mean = statistics.mean(xs)
median = statistics.median(xs)
stdev = statistics.stdev(xs) if len(xs) > 1 else 0.0
# Drop the first sample (warm-up) and last sample (partial second) if we
# have enough data to afford it.
trimmed = xs[1:-1] if len(xs) >= 5 else xs
tmean = statistics.mean(trimmed)
print(f"label={label}  mean={mean:.1f}  trimmed_mean={tmean:.1f}  "
      f"median={median:.1f}  min={min(xs):.1f}  max={max(xs):.1f}  "
      f"stdev={stdev:.1f}  n={len(xs)}")
PY
