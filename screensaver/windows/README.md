# Matrix Screen Saver

Matrix is a native Windows screen saver using the shared Rust/WGPU renderer.

## Build

Run PowerShell on Windows:

```powershell
cd screensaver\windows\matrix
.\release.ps1
```

The package is written to `matrix\dist\Matrix.scr.zip`.

## Test

```powershell
.\Matrix.scr /s
.\Matrix.scr /c
.\Matrix.scr /p 123456
```

The `/p` argument is normally supplied by the Windows Screen Saver Settings
dialog and is not intended for manual use.

## Install

Extract `Matrix.scr`, right-click it, and select **Install**. Configuration is
stored for the current user under `HKCU\Software\Arctan95\Matrix`.
