#!/usr/bin/env bash
# Build a PGO optimized android APK.
#
# The end user APK's .so wraps the same rustboyadvance-core CPU/GPU/bus
# code that fps_bench exercises, so profile data collected from
# running fps_bench on device transfers cleanly to the JNI shared
# library. That lets us skip the awkward "run APK through replay on
# device" flow and reuse scripts/build_pgo_android.sh's profdata
# directly.
#
# Pipeline:
#   1. Run scripts/build_pgo_android.sh (if profdata doesn't already
#      exist) to collect arm64 profraws from fps_bench running on
#      the device.
#   2. Merge profraws into target/pgo-android/merged.profdata.
#   3. Run Gradle assembleDebug with
#      RUSTFLAGS=-Cprofile-use=<abs profdata>, so cargoBuild's rust
#      compile for aarch64-linux-android + x86_64-linux-android uses
#      the profile.
#   4. Copy the resulting APK to the Windows drop dir with a
#      pgo-<git-sha> suffix so it's obvious which APK is the PGO one.
set -euo pipefail

PGO_DIR="${PGO_DIR:-$PWD/target/pgo-android}"
PROFDATA="$PGO_DIR/merged.profdata"
ANDROID_DIR="platform/android"
APK_OUT_DIR="$ANDROID_DIR/app/build/outputs/apk/debug"
WIN_DROP_DIR="${WIN_DROP_DIR:-/mnt/c/dev/pokeemerlad}"

SHA="$(git rev-parse --short HEAD)"

if [[ ! -f "$PROFDATA" ]]; then
  echo "[apk-pgo] profdata missing, running build_pgo_android.sh first" >&2
  scripts/build_pgo_android.sh
fi
[[ -f "$PROFDATA" ]] || { echo "[apk-pgo] still no profdata after bootstrap" >&2; exit 1; }

echo "[apk-pgo] profdata: $PROFDATA ($(stat -c%s "$PROFDATA") bytes)" >&2

# Clean the android arm + x86_64 target dirs so the profile-use pass
# actually re-emits codegen. cargo caches by RUSTFLAGS hash so in
# theory not required, but being defensive after burning time on
# Cranelift experiments.
cargo clean --target aarch64-linux-android -p fps_bench -p rustboyadvance-core \
  -p arm7tdmi -p rustboyadvance-jni --quiet 2>/dev/null || true
cargo clean --target x86_64-linux-android  -p fps_bench -p rustboyadvance-core \
  -p arm7tdmi -p rustboyadvance-jni --quiet 2>/dev/null || true

echo "[apk-pgo] Gradle assembleDebug with profile-use" >&2
(
  cd "$ANDROID_DIR"
  RUSTFLAGS="-Cprofile-use=$PROFDATA" \
  ANDROID_HOME=/usr/lib/android-sdk \
  ANDROID_NDK_HOME=/usr/lib/android-sdk/ndk/27.1.12297006 \
    ./gradlew assembleDebug --no-daemon 2>&1 | tail -20
)

SRC="$APK_OUT_DIR/app-debug.apk"
if [[ ! -f "$SRC" ]]; then
  echo "[apk-pgo] APK not found at $SRC" >&2
  exit 1
fi

DEST="$WIN_DROP_DIR/rustdroid-advance-pgo-$SHA-apk.apk"
mkdir -p "$WIN_DROP_DIR"
cp -f "$SRC" "$DEST"
echo "[apk-pgo] APK: $DEST" >&2

# Optional: install it.
if [[ "${INSTALL:-0}" == "1" ]]; then
  ADB="${ADB:-/home/user/linux-platform-tools/adb}"
  "$ADB" install -r "$SRC" 2>&1 | tail -3
fi
