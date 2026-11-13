#!/bin/zsh
# Builds the Rust core for iPhone, iPhone Simulator and Mac, generates the Swift bindings with
# UniFFI, and packages everything as apple/LuntaCore (a local Swift package).
set -euo pipefail
export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"

ROOT=${0:A:h:h}
CORE=$ROOT/core
PKG=$ROOT/apple/LuntaCore
PROFILE=${PROFILE:-release}
TARGETS=(aarch64-apple-ios aarch64-apple-ios-sim aarch64-apple-darwin)

cd $CORE
for t in $TARGETS; do
  echo "› cargo build --$PROFILE --target $t"
  if [[ $t == *ios* ]]; then export IPHONEOS_DEPLOYMENT_TARGET=17.0; else export MACOSX_DEPLOYMENT_TARGET=14.0; fi
  # --no-default-features: the converter (feature "convert") stays out of the Apple static
  # library; the apps keep using the Swift Paper2EpubKit.
  # feature "drive": the Mac bridges its iCloud library to Google Drive (for Android)
  cargo build --$PROFILE --target $t --lib --no-default-features --features drive --quiet
done

echo "› generating Swift bindings"
GEN=$CORE/target/uniffi-swift
rm -rf $GEN && mkdir -p $GEN
cargo run --quiet --no-default-features --features drive --bin uniffi-bindgen -- generate \
  --library $CORE/target/aarch64-apple-darwin/$PROFILE/liblunta_core.dylib \
  --language swift --out-dir $GEN

HDR=$CORE/target/uniffi-headers
rm -rf $HDR && mkdir -p $HDR
cp $GEN/lunta_coreFFI.h $HDR/
cp $GEN/lunta_coreFFI.modulemap $HDR/module.modulemap

echo "› creating XCFramework"
rm -rf $PKG/LuntaCoreFFI.xcframework
mkdir -p $PKG/Sources/LuntaCore
xcodebuild -create-xcframework \
  -library $CORE/target/aarch64-apple-ios/$PROFILE/liblunta_core.a -headers $HDR \
  -library $CORE/target/aarch64-apple-ios-sim/$PROFILE/liblunta_core.a -headers $HDR \
  -library $CORE/target/aarch64-apple-darwin/$PROFILE/liblunta_core.a -headers $HDR \
  -output $PKG/LuntaCoreFFI.xcframework >/dev/null
cp $GEN/lunta_core.swift $PKG/Sources/LuntaCore/LuntaCore.swift
echo "✓ $PKG"
