# Matrix Screen Saver

Matrix is a native macOS screen saver built on the same Rust/WGPU renderer as the desktop app.

## Build

Build the `.saver` bundle from source:

```sh
cd screensaver/macos/matrix
./release.sh
```

The release script generates the Xcode project with XcodeGen, builds `Matrix.saver`, verifies the bundle, and writes `dist/Matrix.saver.zip`.

## Install

Unzip `Matrix.saver.zip`, double-click `Matrix.saver`, then select **Matrix** in **System Settings -> Screen Saver**.

If macOS blocks a locally built copy, clear the quarantine flag:

```sh
xattr -dr com.apple.quarantine Matrix.saver
```

## Options

Screen saver settings are available from **System Settings -> Screen Saver -> Matrix -> Options...**:

- **Version**: choose one of the Matrix presets.
- **Enable mirror effect**: use the mirror rendering effect.
- **Skip intro**: start directly without the loading sequence.
