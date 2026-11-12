#!/bin/zsh
# Builds the Mac app (Release) and attaches it, zipped, to the GitHub release for a tag.
#   scripts/release-mac.sh v0.1.0
# The release is made by .github/workflows/release.yml when the tag is pushed; this waits for it.
# The app is signed for development only (not Developer ID / notarized), so on other Macs it opens
# with right-click > Open the first time.
set -euo pipefail
tag=${1:?usage: scripts/release-mac.sh <tag, e.g. v0.1.0>}
root=${0:A:h:h}
cd "$root/apple"

until gh release view "$tag" >/dev/null 2>&1; do echo "waiting for release $tag…"; sleep 10; done

xcodegen generate -q
xcodebuild -project Lunta.xcodeproj -scheme LuntaMac -configuration Release -derivedDataPath build/dd build -quiet
out="$root/apple/build/lunta-mac-$tag.zip"
rm -f "$out"
ditto -c -k --sequesterRsrc --keepParent build/dd/Build/Products/Release/Lunta.app "$out"
gh release upload "$tag" "$out" --clobber
echo "attached $(basename "$out") to $tag"
