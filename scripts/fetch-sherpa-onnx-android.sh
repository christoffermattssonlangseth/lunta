#!/usr/bin/env bash
# Puts sherpa-onnx's C API and ONNX Runtime (the natural voice's engine, core/src/tts) into the
# Android app: android/app/src/main/jniLibs/<abi>/libsherpa-onnx-c-api.so and libonnxruntime.so,
# from sherpa-onnx's own release (pinned by SHA-256), not committed. The core loads them at run time;
# the voice's model is downloaded by the app when asked for, never shipped. Run by
# scripts/build-core-android.sh; ABIS as there (default arm64-v8a).
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
JNI=$ROOT/android/app/src/main/jniLibs
ABIS=${ABIS:-arm64-v8a}
VERSION=1.13.8
SHA256=2ff63469a71cb6009aa2e3ed5f4a670f8abdcbe4bb9ffd23776afc792a6b4f44
NAME=sherpa-onnx-v$VERSION-android.tar.bz2
CACHE=$ROOT/android/.cache/sherpa-onnx
DIR=$CACHE/v$VERSION

if [[ ! -d $DIR/jniLibs ]]; then
  mkdir -p "$CACHE"
  echo "  downloading $NAME"
  curl -fsSL "https://github.com/k2-fsa/sherpa-onnx/releases/download/v$VERSION/$NAME" -o "$CACHE/$NAME"
  got=$( (command -v sha256sum >/dev/null && sha256sum "$CACHE/$NAME" || shasum -a 256 "$CACHE/$NAME") | cut -d' ' -f1)
  if [[ $got != "$SHA256" ]]; then echo "$NAME: unexpected SHA-256 $got" >&2; rm -f "$CACHE/$NAME"; exit 1; fi
  rm -rf "$DIR.tmp" && mkdir -p "$DIR.tmp"
  tar -xjf "$CACHE/$NAME" -C "$DIR.tmp"
  rm -rf "$DIR" && mv "$DIR.tmp" "$DIR"
  rm -f "$CACHE/$NAME"
fi
for abi in $ABIS; do
  mkdir -p "$JNI/$abi"
  for lib in libsherpa-onnx-c-api.so libonnxruntime.so; do
    cp "$DIR/jniLibs/$abi/$lib" "$JNI/$abi/$lib"
  done
done
echo "  sherpa-onnx $VERSION: $(for abi in $ABIS; do echo -n "$abi "; done)"
