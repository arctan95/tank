#!/bin/sh
set -eu
cd "$(dirname "$0")"

cleanup() {
    rm -rf build matrix.xcodeproj
}

trap cleanup EXIT

command -v xcodegen >/dev/null || {
    echo "xcodegen is required. Install it with: brew install xcodegen"
    exit 1
}

echo "==> Generating Xcode project"
cleanup
xcodegen generate

echo "==> Building Matrix.saver"
xcodebuild -project matrix.xcodeproj \
           -scheme Matrix \
           -configuration Release \
           -derivedDataPath build \
           build \
           -quiet

SAVER=build/Build/Products/Release/Matrix.saver

echo "==> Verifying bundle"
test -d "$SAVER"
test -f "$SAVER/Contents/Resources/thumbnail.png"
test -f "$SAVER/Contents/Resources/thumbnail@2x.png"
codesign --verify --deep --strict --verbose=2 "$SAVER"

echo "==> Zipping"
mkdir -p dist
rm -f dist/Matrix.saver.zip
ditto -c -k --norsrc --keepParent "$SAVER" dist/Matrix.saver.zip

echo "==> Done: dist/Matrix.saver.zip"
