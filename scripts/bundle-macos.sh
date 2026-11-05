#!/bin/zsh
# Build the desktop app in release mode and assemble "target/Virtual Camera Bender.app".
#
# The bundle holds the vcb-app binary, the camera files, the shipped presets and the
# sample photo (in Contents/Resources, where the app looks when it is not started from
# the repository), an icon drawn by crates/vcb_app/examples/icon.rs and turned into
# .icns with iconutil, and an Info.plist. Finder starts apps with a minimal PATH, so
# the plist adds the usual Homebrew and MacPorts folders, where ffmpeg lives, for
# video. Needs only the Rust toolchain and the tools macOS ships.
#
# Usage: scripts/bundle-macos.sh        (then: open "target/Virtual Camera Bender.app")
set -eu
cd "$(dirname "$0")/.."
root=$PWD
target=${CARGO_TARGET_DIR:-$root/target}
name="Virtual Camera Bender"
app="$target/$name.app"
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)

cargo build --release -q -p vcb_app
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cargo run --release -q -p vcb_app --example icon -- "$work/AppIcon.iconset" > /dev/null

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources/samples"
iconutil -c icns "$work/AppIcon.iconset" -o "$app/Contents/Resources/AppIcon.icns"
cp "$target/release/vcb-app" "$app/Contents/MacOS/vcb-app"
cp -R cameras presets "$app/Contents/Resources/"
cp samples/sample-01.jpg "$app/Contents/Resources/samples/"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleDisplayName</key>
    <string>$name</string>
    <key>CFBundleExecutable</key>
    <string>vcb-app</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>CFBundleIdentifier</key>
    <string>com.resonantcircuits.virtual-camera-bender</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>$name</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$version</string>
    <key>CFBundleVersion</key>
    <string>$version</string>
    <key>LSApplicationCategoryType</key>
    <string>public.app-category.photography</string>
    <key>LSEnvironment</key>
    <dict>
        <key>PATH</key>
        <string>/opt/homebrew/bin:/usr/local/bin:/opt/local/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
    </dict>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSSupportsAutomaticGraphicsSwitching</key>
    <true/>
</dict>
</plist>
PLIST

plutil -lint "$app/Contents/Info.plist" > /dev/null
echo "$app"
