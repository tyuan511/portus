#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "Usage: $0 <arm64-binary> <version> <output-directory>" >&2
  exit 2
fi

binary=$1
version=$2
output_dir=$3

if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Expected a version in X.Y.Z format, got $version." >&2
  exit 2
fi
if [[ "$(lipo -archs "$binary")" != arm64 ]]; then
  echo "Expected an arm64-only macOS binary at $binary." >&2
  exit 1
fi

repo_root=$(cd "$(dirname "$0")/.." && pwd)
mkdir -p "$output_dir"
staging=$(mktemp -d "$output_dir/.portus-package.XXXXXXXX")
trap 'rm -rf "$staging"' EXIT

image_root="$staging/image"
app="$image_root/Portus.app"
contents="$app/Contents"
iconset="$staging/Portus.iconset"

mkdir -p "$contents/MacOS" "$contents/Resources" "$iconset"
install -m 755 "$binary" "$contents/MacOS/portus"
ln -s /Applications "$image_root/Applications"

for size in 16 32 128 256 512; do
  double=$((size * 2))
  sips -z "$size" "$size" "$repo_root/assets/logo.png" \
    --out "$iconset/icon_${size}x${size}.png" >/dev/null
  sips -z "$double" "$double" "$repo_root/assets/logo.png" \
    --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$contents/Resources/Portus.icns"

cat > "$contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDisplayName</key>
  <string>Portus</string>
  <key>CFBundleExecutable</key>
  <string>portus</string>
  <key>CFBundleIconFile</key>
  <string>Portus.icns</string>
  <key>CFBundleIdentifier</key>
  <string>com.github.tyuan511.portus</string>
  <key>CFBundleName</key>
  <string>Portus</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>$version</string>
  <key>CFBundleVersion</key>
  <string>$version</string>
  <key>LSApplicationCategoryType</key>
  <string>public.app-category.developer-tools</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
PLIST

plutil -lint "$contents/Info.plist"
codesign --force --sign - "$app"
codesign --verify --strict "$app"

name="Portus-v${version}-macos-arm64.dmg"
hdiutil create -quiet -srcfolder "$image_root" -volname Portus -fs HFS+ -format UDZO "$staging/$name"
hdiutil verify -quiet "$staging/$name"
mv -f "$staging/$name" "$output_dir/$name"
printf 'Created %s\n' "$output_dir/$name"
