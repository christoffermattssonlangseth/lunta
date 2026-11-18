#!/bin/bash
# The update feeds on a GitHub release, which the apps read from the latest release:
#   latest.json   Windows and Linux (AppImage), the Tauri updater: made from the installers' .sig
#                 files that desktop.yml attaches (release.yml runs this after the desktop builds)
#   appcast.xml   Mac, Sparkle: the notarized zip signed with the EdDSA key in this Mac's login
#                 Keychain (scripts/release-mac.sh runs this after attaching the zip)
# Android reads the release itself (GitHub's API), so it needs no feed.
#
#   scripts/update-feeds.sh desktop v1.0.6          latest.json
#   scripts/update-feeds.sh mac v1.0.6 [Lunta.zip]  appcast.xml (the zip on the release if none given)
#   scripts/update-feeds.sh all v1.0.6              both: after editing the release notes
#                                                   (gh release edit), to put them in the feeds too
# DRY_RUN=1 prints the feed instead of attaching it; FEEDS_LOCAL_DIR=<dir> takes the .sig files and
# notes.md from that folder instead of the release (the test in desktop/src-tauri/src/updates.rs).
# The notes in the feeds are the release notes above "## Which file do I need?", without the
# template's "Lunta is a reference manager…" line.
set -euo pipefail
what=${1:?usage: scripts/update-feeds.sh desktop|mac|all <tag> [mac zip]}
tag=${2:?usage: scripts/update-feeds.sh desktop|mac|all <tag> [mac zip]}
zip=${3:-}
repo=${GITHUB_REPOSITORY:-christoffermattssonlangseth/lunta}
root=$(cd "$(dirname "$0")/.." && pwd)
v=${tag#v}
dl="https://github.com/$repo/releases/download/$tag"
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
attach() {
  if [[ -n ${DRY_RUN:-} ]]; then cat "$1"; return; fi
  gh release upload "$tag" -R "$repo" --clobber "$1"
  echo "attached $(basename "$1") to $tag ($2)"
}

# What's new in this version, as Markdown
notes() {
  if [[ -n ${FEEDS_LOCAL_DIR:-} ]]; then cat "$FEEDS_LOCAL_DIR/notes.md"
  else gh release view "$tag" -R "$repo" --json body -q .body; fi | tr -d '\r' | awk '
    /^## Which file do I need\?/ { exit }
    /^Lunta is a reference manager for scientific papers/ { next }
    { line[n++] = $0 }
    END {  # without blank lines at either end
      s = 0; while (s < n && line[s] ~ /^[ \t]*$/) s++
      e = n - 1; while (e >= s && line[e] ~ /^[ \t]*$/) e--
      for (i = s; i <= e; i++) print line[i]
    }'
}

desktop() {
  # the signatures desktop.yml attached next to the installers
  if [[ -n ${FEEDS_LOCAL_DIR:-} ]]; then mkdir -p "$tmp/sig"; cp "$FEEDS_LOCAL_DIR"/*.sig "$tmp/sig/"
  else gh release download "$tag" -R "$repo" -D "$tmp/sig" -p "Lunta-$v-*.sig"; fi
  sig() { [[ -f "$tmp/sig/$1.sig" ]] || { echo "missing $1.sig on $tag (desktop build without TAURI_SIGNING_PRIVATE_KEY?)" >&2; exit 1; }; cat "$tmp/sig/$1.sig"; }
  local nsis="Lunta-$v-Windows-Setup.exe" msi="Lunta-$v-Windows.msi" appimage="Lunta-$v-Linux.AppImage"
  # {os}-{arch}-{installer} is what the updater looks for first; {os}-{arch} is the fallback.
  # A .deb install never installs from here: the app sees it isn't an AppImage and offers the
  # release page instead (desktop/src-tauri/src/updates.rs).
  jq -n --arg version "$v" --arg notes "$(notes)" --arg date "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    --arg nsis_url "$dl/$nsis" --arg nsis_sig "$(sig "$nsis")" \
    --arg msi_url "$dl/$msi" --arg msi_sig "$(sig "$msi")" \
    --arg ai_url "$dl/$appimage" --arg ai_sig "$(sig "$appimage")" '{
      version: $version, notes: $notes, pub_date: $date,
      platforms: {
        "windows-x86_64":          {url: $nsis_url, signature: $nsis_sig},
        "windows-x86_64-nsis":     {url: $nsis_url, signature: $nsis_sig},
        "windows-x86_64-msi":      {url: $msi_url,  signature: $msi_sig},
        "linux-x86_64":            {url: $ai_url,   signature: $ai_sig},
        "linux-x86_64-appimage":   {url: $ai_url,   signature: $ai_sig}
      }}' > "$tmp/latest.json"
  attach "$tmp/latest.json" "version $v"
}

# Sparkle's tools come with its Swift package (artifacts/sparkle/Sparkle/bin)
sparkle_bin() {
  local d
  for d in "${SPARKLE_BIN:-}" "$root/apple/build/SourcePackages/artifacts/sparkle/Sparkle/bin" \
           "$root/apple/build/dd/SourcePackages/artifacts/sparkle/Sparkle/bin"; do
    [[ -n $d && -x $d/sign_update ]] && { echo "$d"; return; }
  done
  d=$(find ~/Library/Developer/Xcode/DerivedData -path '*/artifacts/sparkle/Sparkle/bin/sign_update' 2>/dev/null | head -1)
  [[ -n $d ]] && { dirname "$d"; return; }
  echo "Sparkle's sign_update not found: build LuntaMac once (or set SPARKLE_BIN)" >&2; exit 1
}

mac() {
  local name="Lunta-$v-Mac-AppleSilicon.zip"
  if [[ -z $zip ]]; then
    gh release download "$tag" -R "$repo" -D "$tmp" -p "$name"
    zip="$tmp/$name"
  fi
  unzip -p "$zip" Lunta.app/Contents/Info.plist > "$tmp/Info.plist"
  local build short minos
  build=$(plutil -extract CFBundleVersion raw "$tmp/Info.plist")
  short=$(plutil -extract CFBundleShortVersionString raw "$tmp/Info.plist")
  minos=$(plutil -extract LSMinimumSystemVersion raw "$tmp/Info.plist")
  [[ $short == "$v" ]] || { echo "the zip is Lunta $short, not $v" >&2; exit 1; }
  # sparkle:edSignature="…" length="…", with the private key from the login Keychain
  local signed
  signed=$("$(sparkle_bin)/sign_update" --account lunta "$zip")
  [[ $signed == *edSignature=* ]] || { echo "sign_update failed: $signed" >&2; exit 1; }
  local html
  html=$(notes | gh api markdown -F text=@- -f mode=gfm -f context="$repo")
  html=${html//]]>/]]&gt;}
  cat > "$tmp/appcast.xml" <<XML
<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle" xmlns:dc="http://purl.org/dc/elements/1.1/">
  <channel>
    <title>Lunta</title>
    <link>https://github.com/$repo/releases</link>
    <item>
      <title>Lunta $v</title>
      <pubDate>$(LC_ALL=C date -u '+%a, %d %b %Y %H:%M:%S +0000')</pubDate>
      <sparkle:version>$build</sparkle:version>
      <sparkle:shortVersionString>$v</sparkle:shortVersionString>
      <sparkle:minimumSystemVersion>$minos</sparkle:minimumSystemVersion>
      <sparkle:fullReleaseNotesLink>https://github.com/$repo/releases/tag/$tag</sparkle:fullReleaseNotesLink>
      <description><![CDATA[$html]]></description>
      <enclosure url="$dl/$name" type="application/octet-stream" $signed />
    </item>
  </channel>
</rss>
XML
  xmllint --noout "$tmp/appcast.xml"
  attach "$tmp/appcast.xml" "Lunta $v, build $build"
}

case $what in
  desktop) desktop ;;
  mac) mac ;;
  all) desktop; mac ;;
  *) echo "usage: scripts/update-feeds.sh desktop|mac|all <tag> [mac zip]" >&2; exit 1 ;;
esac
