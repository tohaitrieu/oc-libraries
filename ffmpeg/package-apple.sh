#!/usr/bin/env bash
# Wrap the FFmpeg dylibs as .xcframework bundles an iOS app can embed.
#
# A bare .dylib links fine and then is not there at run time: CocoaPods embeds and signs frameworks,
# not loose libraries, and iOS refuses to load anything it did not embed. So each library becomes a
# framework, and device and simulator slices go into one .xcframework — mixing the two in a single
# binary is what App Store review rejects.
#
# Install names are rewritten to @rpath, both the libraries' own and the references between them.
# FFmpeg bakes the build machine's absolute prefix in; on any other machine that path does not exist.
#
# Run ./build-mobile.sh for both ios and ios-sim first.
set -euo pipefail
cd "$(dirname "$0")"

LIBS=(avcodec avdevice avfilter avformat avutil swresample swscale)
MIN_OS=16.4
OUT="$PWD/xcframework"
rm -rf "$OUT"; mkdir -p "$OUT"

# The versioned file name a library carries, e.g. libavcodec.63.dylib.
soname() {
  basename "$(readlink "$1/prefix/$2/lib/lib$3.dylib" 2>/dev/null || echo "lib$3.dylib")"
}

make_framework() {
  local slice="$1" lib="$2" dir="$3"
  local prefix="$PWD/prefix/$slice"
  local real
  real="$(cd "$prefix/lib" && readlink "lib$lib.dylib")"
  local fw="$dir/lib$lib.framework"
  mkdir -p "$fw"
  cp "$prefix/lib/$real" "$fw/lib$lib"

  cat > "$fw/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleExecutable</key><string>lib$lib</string>
  <key>CFBundleIdentifier</key><string>org.ffmpeg.lib$lib</string>
  <key>CFBundleName</key><string>lib$lib</string>
  <key>CFBundlePackageType</key><string>FMWK</string>
  <key>CFBundleShortVersionString</key><string>1.0</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>MinimumOSVersion</key><string>$MIN_OS</string>
</dict></plist>
PLIST

  install_name_tool -id "@rpath/lib$lib.framework/lib$lib" "$fw/lib$lib"
  # Point every sibling reference at its framework too, or the loader goes looking for the build
  # machine's own directories.
  for other in "${LIBS[@]}"; do
    local old
    old="$(otool -L "$fw/lib$lib" | awk -v n="lib$other." '$1 ~ n {print $1; exit}')"
    [ -n "$old" ] || continue
    install_name_tool -change "$old" "@rpath/lib$other.framework/lib$other" "$fw/lib$lib"
  done
}

for lib in "${LIBS[@]}"; do
  work="$(mktemp -d)"
  mkdir -p "$work/device" "$work/sim"
  make_framework ios "$lib" "$work/device"
  make_framework ios-sim "$lib" "$work/sim"
  xcodebuild -create-xcframework \
    -framework "$work/device/lib$lib.framework" \
    -framework "$work/sim/lib$lib.framework" \
    -output "$OUT/lib$lib.xcframework" >/dev/null
  rm -rf "$work"
  echo "→ lib$lib.xcframework"
done

ls "$OUT"
