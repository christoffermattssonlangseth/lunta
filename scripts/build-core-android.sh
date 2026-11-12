#!/usr/bin/env bash
# Builds the Rust core for Android and prepares what the Gradle project in android/ needs:
#   android/app/src/main/jniLibs/<abi>/liblunta_core.so   the core, with features convert, pdf, drive
#   android/app/src/main/jniLibs/<abi>/libpdfium.so       PDFium (bblanchon/pdfium-binaries), downloaded, not committed
#   android/app/src/main/assets/licenses/pdfium*          PDFium's licences (pdfium-LICENSE.txt is committed, the rest not)
#   android/app/src/main/generated/uniffi/                Kotlin bindings (UniFFI), not committed
#
# Usage: scripts/build-core-android.sh            arm64-v8a only (phones, the arm64 emulator)
#        ABIS="arm64-v8a x86_64" scripts/build-core-android.sh
#        PDFIUM_TAG=chromium/8152 ...             pin a PDFium release (default: latest)
#        PROFILE=debug ...                        faster builds, larger library
# Needs: rustup targets aarch64-linux-android / x86_64-linux-android, cargo-ndk, the Android NDK
# (ANDROID_NDK_HOME, or ANDROID_HOME/ndk/<version>).
set -euo pipefail
export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
CORE=$ROOT/core
APP=$ROOT/android/app
JNI=$APP/src/main/jniLibs
ABIS=${ABIS:-arm64-v8a}
PROFILE=${PROFILE:-release}

if [[ -z "${ANDROID_NDK_HOME:-}" ]]; then
  SDK=${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}
  [[ -n "$SDK" && -d "$SDK/ndk" ]] || { echo "set ANDROID_NDK_HOME (or ANDROID_HOME with an installed ndk;<version>)" >&2; exit 1; }
  ANDROID_NDK_HOME=$(ls -d "$SDK"/ndk/*/ | sort -V | tail -1)
  ANDROID_NDK_HOME=${ANDROID_NDK_HOME%/}
fi
export ANDROID_NDK_HOME
command -v cargo-ndk >/dev/null || { echo "install cargo-ndk: cargo install cargo-ndk" >&2; exit 1; }

targets=(); pdfium_name=(); triple=()
for abi in $ABIS; do
  targets+=(-t "$abi")
  case $abi in
    arm64-v8a) triple+=(aarch64-linux-android); pdfium_name+=(arm64) ;;
    x86_64)    triple+=(x86_64-linux-android);  pdfium_name+=(x64) ;;
    *) echo "unsupported ABI $abi (arm64-v8a, x86_64)" >&2; exit 1 ;;
  esac
done

echo "› cargo ndk build (--$PROFILE, $ABIS)"
cd "$CORE"
flag=--release; [[ $PROFILE == debug ]] && flag=
cargo ndk "${targets[@]}" -o "$JNI" build $flag --lib --features convert,pdf,drive

echo "› PDFium"
CACHE=$ROOT/android/.cache/pdfium
mkdir -p "$CACHE"
tag=${PDFIUM_TAG:-}
i=0
for abi in $ABIS; do
  name=${pdfium_name[$i]}; i=$((i + 1))
  dir=$CACHE/${tag//\//_}-$name
  if [[ ! -f $dir/lib/libpdfium.so ]]; then
    if [[ -z $tag ]]; then url=https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-android-$name.tgz
    else url=https://github.com/bblanchon/pdfium-binaries/releases/download/$tag/pdfium-android-$name.tgz; fi
    echo "  downloading $url"
    rm -rf "$dir"; mkdir -p "$dir"
    curl -fsSL "$url" -o "$dir.tgz"
    tar -xzf "$dir.tgz" -C "$dir"
    rm -f "$dir.tgz"
  fi
  mkdir -p "$JNI/$abi"
  cp "$dir/lib/libpdfium.so" "$JNI/$abi/libpdfium.so"
  # cargo-ndk also copies a stray dependency cdylib (libpdfium_render-*.so); only these two belong in the app
  find "$JNI/$abi" -name '*.so' ! -name liblunta_core.so ! -name libpdfium.so -delete
  # PDFium's own licence is committed (assets/licenses/pdfium-LICENSE.txt); the licences of the
  # libraries inside the binary (FreeType, ICU, ...) are shipped in the app too but not committed
  licenses=$APP/src/main/assets/licenses
  mkdir -p "$licenses/pdfium"
  cp "$dir/LICENSE" "$licenses/pdfium-LICENSE.txt"
  if [[ -d $dir/licenses ]]; then
    for f in "$dir"/licenses/*; do n=$(basename "$f"); n=${n%.txt}; cp "$f" "$licenses/pdfium/$n.txt"; done
  fi
done

echo "› Kotlin bindings"
GEN=$APP/src/main/generated/uniffi
rm -rf "$GEN" && mkdir -p "$GEN"
first=${triple[0]}
cd "$CORE"
cargo run --quiet --bin uniffi-bindgen -- generate \
  --library "$CORE/target/$first/$PROFILE/liblunta_core.so" \
  --language kotlin --out-dir "$GEN" --no-format
# the generated file imports com.sun.jna.Library (unused) which shadows our exported class "Library"
sed -i.bak '/^import com.sun.jna.Library$/d' "$GEN/uniffi/lunta_core/lunta_core.kt" && rm -f "$GEN/uniffi/lunta_core/lunta_core.kt.bak"
echo "✓ jniLibs: $(ls "$JNI")  bindings: $(find "$GEN" -name '*.kt' | wc -l | tr -d ' ') file(s)"
