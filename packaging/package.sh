#!/usr/bin/env bash
# Packs a release build of VNRevival for one system, with the Noto Sans JP
# font, the license and the manual. Used by .github/workflows/release.yml;
# it can also be run by hand (Git Bash on Windows).
#
#   packaging/package.sh windows <exe>               -> dist/VNRevival-<v>-windows-x64.zip
#   packaging/package.sh linux   <binary>            -> dist/VNRevival-<v>-linux-x64.tar.gz
#                                                       and .AppImage
#   packaging/package.sh macos   <universal binary>  -> dist/VNRevival-<v>-macos.dmg
#
# The version comes from the workspace's Cargo.toml.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DIST="$ROOT/dist"
WORK="$ROOT/target/package"

# Noto Sans JP, pinned to a commit of github.com/google/fonts.
FONT_COMMIT=66a36c8c94b1a5d992ee4e7f392fccfe4945767c
FONT_URL="https://raw.githubusercontent.com/google/fonts/$FONT_COMMIT/ofl/notosansjp"
FONT_SHA256=c2f3b4d463500a2ddcd3849cded1fceeb9fd6d1c32e6cbecd568453ba50fc68f
OFL_SHA256=1c05c68c34f9708415aada51f17e1b0092d2cea709bf4a94cd38114f9e73d7d9

APPIMAGETOOL_URL=https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -n 1)"

die() {
    echo "package.sh: $*" >&2
    exit 1
}

sha256() {
    if command -v sha256sum > /dev/null; then
        sha256sum "$1" | cut -d ' ' -f 1
    else
        shasum -a 256 "$1" | cut -d ' ' -f 1
    fi
}

# Downloads the font once into target/package/fonts and checks it.
fetch_fonts() {
    local dir="$WORK/fonts"
    mkdir -p "$dir"
    if [ ! -f "$dir/NotoSansJP-VF.ttf" ]; then
        curl -sSfL -o "$dir/NotoSansJP-VF.ttf" "$FONT_URL/NotoSansJP%5Bwght%5D.ttf"
    fi
    if [ ! -f "$dir/OFL.txt" ]; then
        curl -sSfL -o "$dir/OFL.txt" "$FONT_URL/OFL.txt"
    fi
    [ "$(sha256 "$dir/NotoSansJP-VF.ttf")" = "$FONT_SHA256" ] || die "NotoSansJP-VF.ttf: wrong checksum"
    [ "$(sha256 "$dir/OFL.txt")" = "$OFL_SHA256" ] || die "OFL.txt: wrong checksum"
}

# Copies the fonts, the license and the manual into $1 (fonts into $2,
# default $1/fonts).
add_common() {
    local dest="$1" fonts="${2:-$1/fonts}"
    mkdir -p "$fonts"
    cp "$WORK/fonts/NotoSansJP-VF.ttf" "$fonts/"
    cp "$WORK/fonts/OFL.txt" "$fonts/OFL.txt"
    cp "$ROOT/LICENSE" "$dest/LICENSE.txt"
    cp "$ROOT/docs/MANUAL.md" "$dest/MANUAL.md"
}

package_windows() {
    local exe="$1" name="VNRevival-$VERSION-windows-x64"
    local stage="$WORK/$name"
    rm -rf "$stage" && mkdir -p "$stage"
    cp "$exe" "$stage/VNRevival.exe"
    add_common "$stage"
    rm -f "$DIST/$name.zip"
    if command -v 7z > /dev/null; then
        (cd "$WORK" && 7z a -tzip -mx=9 "$DIST/$name.zip" "$name" > /dev/null)
    else
        (cd "$WORK" && pwsh -NoProfile -Command "Compress-Archive -Path '$name' -DestinationPath '$(cygpath -w "$DIST/$name.zip")'")
    fi
    echo "$DIST/$name.zip"
}

package_linux() {
    local bin="$1" name="VNRevival-$VERSION-linux-x64"
    local stage="$WORK/$name"
    rm -rf "$stage" && mkdir -p "$stage"
    install -m 755 "$bin" "$stage/vnrevival"
    add_common "$stage"
    tar -C "$WORK" -czf "$DIST/$name.tar.gz" "$name"
    echo "$DIST/$name.tar.gz"

    # The AppImage: the usual /usr layout, started through AppRun.
    local appdir="$WORK/VNRevival.AppDir"
    rm -rf "$appdir"
    mkdir -p "$appdir/usr/bin" "$appdir/usr/share/vnrevival" \
        "$appdir/usr/share/applications" "$appdir/usr/share/icons/hicolor/256x256/apps"
    install -m 755 "$bin" "$appdir/usr/bin/vnrevival"
    add_common "$appdir/usr/share/vnrevival"
    cp "$ROOT/packaging/linux/vnrevival.desktop" "$appdir/usr/share/applications/"
    cp "$ROOT/packaging/linux/vnrevival.desktop" "$appdir/"
    cp "$ROOT/assets/icon/vnrevival-256.png" "$appdir/usr/share/icons/hicolor/256x256/apps/vnrevival.png"
    cp "$ROOT/assets/icon/vnrevival-256.png" "$appdir/vnrevival.png"
    ln -s usr/bin/vnrevival "$appdir/AppRun"

    local tool="$WORK/appimagetool"
    if [ ! -x "$tool" ]; then
        curl -sSfL -o "$tool" "$APPIMAGETOOL_URL"
        chmod +x "$tool"
    fi
    rm -f "$DIST/$name.AppImage"
    # Without FUSE (as on CI runners) the tool runs from an extracted copy.
    APPIMAGE_EXTRACT_AND_RUN=1 ARCH=x86_64 "$tool" --no-appstream "$appdir" "$DIST/$name.AppImage"
    echo "$DIST/$name.AppImage"
}

package_macos() {
    local bin="$1" name="VNRevival-$VERSION-macos"
    local stage="$WORK/$name"
    local app="$stage/VNRevival.app"
    rm -rf "$stage"
    mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
    install -m 755 "$bin" "$app/Contents/MacOS/vnrevival"
    sed "s/@VERSION@/$VERSION/g" "$ROOT/packaging/macos/Info.plist" > "$app/Contents/Info.plist"
    cp "$ROOT/assets/icon/vnrevival.icns" "$app/Contents/Resources/"
    add_common "$app/Contents/Resources"
    # Not signed with a developer identity: an ad-hoc signature, which
    # Apple Silicon needs to run the program at all.
    codesign --force --deep --sign - "$app"
    cp "$ROOT/docs/MANUAL.md" "$stage/MANUAL.md"
    ln -s /Applications "$stage/Applications"
    rm -f "$DIST/$name.dmg"
    hdiutil create -volname "VNRevival $VERSION" -srcfolder "$stage" -ov -format UDZO "$DIST/$name.dmg" > /dev/null
    echo "$DIST/$name.dmg"
}

[ $# -eq 2 ] || die "usage: package.sh windows|linux|macos <binary>"
[ -n "$VERSION" ] || die "no version in Cargo.toml"
[ -f "$2" ] || die "$2: not found"
mkdir -p "$DIST" "$WORK"
fetch_fonts
case "$1" in
    windows) package_windows "$2" ;;
    linux) package_linux "$2" ;;
    macos) package_macos "$2" ;;
    *) die "unknown system $1" ;;
esac
