#!/bin/zsh
# Builds the Mac app for distribution, signed with Developer ID and notarized by Apple, and
# attaches it (zipped) to the GitHub release for a tag:
#   scripts/release-mac.sh v0.1.0             build, notarize, attach to the release
#   scripts/release-mac.sh --check            build and notarize only (nothing is published)
# The release is made by .github/workflows/release.yml when the tag is pushed; this waits for it.
#
# Signing and notarizing go through the Apple account Xcode is signed in to (Xcode > Settings >
# Accounts): the first run lets Xcode create the Developer ID certificate if there is none.
set -euo pipefail
log() { echo "  (full output: $build/xcodebuild.log)"; }
tag=${1:?usage: scripts/release-mac.sh <tag, e.g. v0.1.0> | --check}
root=${0:A:h:h}
team=7R6QL53THM
build="$root/apple/build/release-mac"
cd "$root/apple"

if [[ $tag != --check ]]; then
  # the release is made by GitHub (release.yml) a minute after the tag is pushed
  until err=$(gh release view "$tag" 2>&1 >/dev/null); do
    echo "waiting for release $tag… (gh: ${err%%$'\n'*})"
    sleep 10
  done
fi

rm -rf "$build"; mkdir -p "$build"
cat > "$build/ExportOptions.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>method</key><string>developer-id</string>
  <key>signingStyle</key><string>automatic</string>
  <key>teamID</key><string>$team</string>
  <key>destination</key><string>upload</string>
</dict></plist>
PLIST

echo "› archive"
xcodegen generate -q
# the compiler's warnings (mostly MLX's Metal shaders) go to a log; errors are shown
# Apple silicon only (ARCHS also for the Swift packages): the Rust core is arm64
xcodebuild -project Lunta.xcodeproj -scheme LuntaMac -configuration Release -destination 'generic/platform=macOS' ARCHS=arm64 \
  -archivePath "$build/Lunta.xcarchive" -allowProvisioningUpdates archive >"$build/xcodebuild.log" 2>&1 ||
  { grep -E " error: |\*\* ARCHIVE" "$build/xcodebuild.log" | tail -20; log; exit 1; }

echo "› sign with Developer ID and send to Apple for notarization"
xcodebuild -exportArchive -archivePath "$build/Lunta.xcarchive" -exportPath "$build/submitted" \
  -exportOptionsPlist "$build/ExportOptions.plist" -allowProvisioningUpdates >>"$build/xcodebuild.log" 2>&1 ||
  { grep -iE "error" "$build/xcodebuild.log" | tail -20; log; exit 1; }

echo "› waiting for Apple to notarize it (usually a few minutes)"
for i in {1..90}; do
  if xcodebuild -exportNotarizedApp -archivePath "$build/Lunta.xcarchive" -exportPath "$build/export" \
       -quiet >/dev/null 2>"$build/notary.log"; then break; fi
  (( i == 90 )) && { echo "not notarized after 30 min:"; cat "$build/notary.log"; exit 1; }
  sleep 20
done
app="$build/export/Lunta.app"
xcrun stapler validate "$app"
spctl --assess --type execute --verbose "$app"
echo "notarized: $app"

[[ $tag == --check ]] && exit 0
out="$build/Lunta-${tag#v}-Mac-AppleSilicon.zip"
ditto -c -k --sequesterRsrc --keepParent "$app" "$out"
gh release upload "$tag" "$out" --clobber
echo "attached $(basename "$out") to $tag"
