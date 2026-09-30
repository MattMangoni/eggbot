#!/bin/zsh
# Builds eggbot.app (release, ad-hoc signed) and installs it in ~/Applications.
set -euo pipefail
cd "${0:a:h}/.."

cargo build --release
app=target/release/eggbot.app
rm -rf $app
mkdir -p $app/Contents/{MacOS,Resources}
cp target/release/eggbot $app/Contents/MacOS/eggbot

# icon: draw once at 1024px, then scale into an iconset
set=target/release/eggbot.iconset
rm -rf $set && mkdir -p $set
swift scripts/icon.swift $set/icon_512x512@2x.png
for s in 16 32 128 256 512; do
  sips -z $s $s $set/icon_512x512@2x.png --out $set/icon_${s}x${s}.png >/dev/null
  sips -z $((s * 2)) $((s * 2)) $set/icon_512x512@2x.png --out $set/icon_${s}x${s}@2x.png >/dev/null
done
iconutil -c icns $set -o $app/Contents/Resources/eggbot.icns

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
cat > $app/Contents/Info.plist <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key><string>com.digitalmaze.eggbot</string>
  <key>CFBundleName</key><string>eggbot</string>
  <key>CFBundleDisplayName</key><string>eggbot</string>
  <key>CFBundleExecutable</key><string>eggbot</string>
  <key>CFBundleIconFile</key><string>eggbot</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

codesign --force --sign - $app
mkdir -p ~/Applications
rm -rf ~/Applications/eggbot.app
cp -R $app ~/Applications/
echo "Installed ~/Applications/eggbot.app"
