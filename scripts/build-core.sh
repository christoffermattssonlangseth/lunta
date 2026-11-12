#!/bin/zsh
# Builds the Rust core for iPhone, iPhone Simulator and Mac, generates the Swift bindings with
# UniFFI, and packages everything as apple/MarginsCore (a local Swift package).
set -euo pipefail
export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"

ROOT=${0:A:h:h}
CORE=$ROOT/core
PKG=$ROOT/apple/MarginsCore
PROFILE=${PROFILE:-release}
TARGETS=(aarch64-apple-ios aarch64-apple-ios-sim aarch64-apple-darwin)

cd $CORE
for t in $TARGETS; do
  echo "› cargo build --$PROFILE --target $t"
  if [[ $t == *ios* ]]; then export IPHONEOS_DEPLOYMENT_TARGET=17.0; else export MACOSX_DEPLOYMENT_TARGET=14.0; fi
  # --no-default-features: the converter (feature "convert") stays out of the Apple static
  # library; the apps keep using the Swift Paper2EpubKit.
  cargo build --$PROFILE --target $t --lib --no-default-features --quiet
done

echo "› generating Swift bindings"
GEN=$CORE/target/uniffi-swift
rm -rf $GEN && mkdir -p $GEN
cargo run --quiet --no-default-features --bin uniffi-bindgen -- generate \
  --library $CORE/target/aarch64-apple-darwin/$PROFILE/libmargins_core.dylib \
  --language swift --out-dir $GEN

HDR=$CORE/target/uniffi-headers
rm -rf $HDR && mkdir -p $HDR
cp $GEN/margins_coreFFI.h $HDR/
cp $GEN/margins_coreFFI.modulemap $HDR/module.modulemap

echo "› creating XCFramework"
rm -rf $PKG/MarginsCoreFFI.xcframework
mkdir -p $PKG/Sources/MarginsCore
xcodebuild -create-xcframework \
  -library $CORE/target/aarch64-apple-ios/$PROFILE/libmargins_core.a -headers $HDR \
  -library $CORE/target/aarch64-apple-ios-sim/$PROFILE/libmargins_core.a -headers $HDR \
  -library $CORE/target/aarch64-apple-darwin/$PROFILE/libmargins_core.a -headers $HDR \
  -output $PKG/MarginsCoreFFI.xcframework >/dev/null
cp $GEN/margins_core.swift $PKG/Sources/MarginsCore/MarginsCore.swift
echo "✓ $PKG"
