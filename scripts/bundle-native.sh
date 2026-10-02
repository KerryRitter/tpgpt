#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release --locked -p tpgpt
python3 scripts/third-party-notices.py
version="$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "tpgpt"))')"
build_dir="${CARGO_TARGET_DIR:-target}/release"
bundle_dir="$PWD/native/bundle"
mkdir -p "$bundle_dir"
case "$(uname -s)" in
  Linux)
    stage="$bundle_dir/linux-tpgpt"
    rm -rf "$stage"
    mkdir -p "$stage/usr/bin" "$stage/usr/share/applications" "$stage/usr/share/doc/tpgpt" "$stage/DEBIAN"
    install -m 755 "$build_dir/tpgpt" "$stage/usr/bin/tpgpt"
    install -m 644 README.md "$stage/usr/share/doc/tpgpt/README.md"
    install -m 644 "$bundle_dir/THIRD-PARTY-NOTICES.txt" "$stage/usr/share/doc/tpgpt/THIRD-PARTY-NOTICES.txt"
    cat > "$stage/usr/share/applications/tpgpt.desktop" <<'DESKTOP'
[Desktop Entry]
Type=Application
Name=TPGPT
Comment=Chat with your local TrainingPeaks training history
Exec=tpgpt
Terminal=false
Categories=Education;Sports;
Icon=applications-education
DESKTOP
    arch="$(dpkg --print-architecture)"
    cat > "$stage/DEBIAN/control" <<CONTROL
Package: tpgpt
Version: $version
Architecture: $arch
Maintainer: TPGPT contributors
Depends: libgtk-3-0, libwebkit2gtk-4.1-0, libegl1, libgl1, libxkbcommon0, libc6 (>= 2.35)
Description: Native TrainingPeaks local-data chat
 Imports personal TrainingPeaks history into SQLite and chats through
 an installed, authenticated Codex or Claude CLI.
CONTROL
    dpkg-deb --build --root-owner-group "$stage" "$bundle_dir/tpgpt_${version}_${arch}.deb"
    portable="$bundle_dir/linux-portable"
    rm -rf "$portable"
    mkdir -p "$portable"
    install -m 755 "$build_dir/tpgpt" "$portable/tpgpt"
    install -m 644 README.md "$portable/README.md"
    install -m 644 "$bundle_dir/THIRD-PARTY-NOTICES.txt" "$portable/THIRD-PARTY-NOTICES.txt"
    tar -czf "$bundle_dir/tpgpt-linux-$(uname -m).tar.gz" -C "$portable" tpgpt README.md THIRD-PARTY-NOTICES.txt
    ;;
  Darwin)
    architecture="$(uname -m)"
    stage="$bundle_dir/macos-$architecture"
    rm -rf "$stage"
    app="$stage/TPGPT.app"
    mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
    install -m 755 "$build_dir/tpgpt" "$app/Contents/MacOS/tpgpt"
    install -m 644 README.md "$app/Contents/Resources/README.md"
    install -m 644 "$bundle_dir/THIRD-PARTY-NOTICES.txt" "$app/Contents/Resources/THIRD-PARTY-NOTICES.txt"
    cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>tpgpt</string>
<key>CFBundleIdentifier</key><string>com.kerryritter.tpgpt</string>
<key>CFBundleName</key><string>TPGPT</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>$version</string>
<key>CFBundleVersion</key><string>$version</string>
<key>LSMinimumSystemVersion</key><string>12.0</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
    codesign --force --sign - "$app"
    codesign --verify --strict "$app"
    ditto -c -k --keepParent "$app" "$bundle_dir/tpgpt-macos-$architecture.zip"
    ln -s /Applications "$stage/Applications"
    hdiutil create -volname TPGPT -srcfolder "$stage" -ov -format UDZO "$bundle_dir/tpgpt-macos-$architecture.dmg"
    ;;
  *)
    echo 'Use scripts/bundle-windows.ps1 on Windows.' >&2
    exit 1
    ;;
esac
echo "Bundles: $bundle_dir"
