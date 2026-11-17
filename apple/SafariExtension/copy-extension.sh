#!/bin/zsh
# Build phase of the Safari extension targets (LuntaSafariMac, LuntaSafari): puts the browser
# extension in extension/ into the extension's bundle, as is (the same JS as Chrome and Firefox).
# Only the manifest is adjusted for Safari:
#   - the version is the app's (MARKETING_VERSION)
#   - background: the service worker only (Safari doesn't take Firefox's "scripts" beside it)
#   - Firefox's browser_specific_settings are dropped
#   - iPhone and iPad: "nativeMessaging", the way requests reach the Lunta app there (no server
#     on the device: background.js hands them to SafariWebExtensionHandler)
set -euo pipefail
src="$SRCROOT/../extension"
dest="$TARGET_BUILD_DIR/$UNLOCALIZED_RESOURCES_FOLDER_PATH"
mkdir -p "$dest/icons"
for f in manifest.json background.js content.js; do cp -f "$src/$f" "$dest/$f"; done
cp -f "$src"/icons/*.png "$dest/icons/"
m="$dest/manifest.json"
plutil -replace version -string "$MARKETING_VERSION" "$m"
plutil -remove browser_specific_settings "$m" 2>/dev/null || true
plutil -remove background.scripts "$m" 2>/dev/null || true
if [[ $PLATFORM_NAME == iphoneos || $PLATFORM_NAME == iphonesimulator ]]; then
  plutil -insert permissions -string nativeMessaging -append "$m"
fi
