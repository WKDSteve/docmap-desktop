docmap-desktop as an installer for each platform.

## Install

Without administrator rights, for one user, and without SmartScreen stopping a downloaded installer: the
`install` script of this release installs exactly this release. It downloads the installer of your system, checks
it against `SHA256SUMS` and runs it silently. Run it in your own terminal (PowerShell not elevated):

```
# Windows
irm https://github.com/WKDSteve/docmap-desktop/releases/download/{tag}/install.ps1 | iex

# Linux x86-64
curl -fsSL https://github.com/WKDSteve/docmap-desktop/releases/download/{tag}/install.sh | sh
```

Type the line into the PowerShell session as it is. Wrapped in a new `powershell -ExecutionPolicy Bypass -c "..."`, Microsoft Defender reports the command line as a trojan (`Trojan:Win32/Commando.B!ml`) and nothing is installed.

The Linux script puts the AppImage in `~/.local/bin/docmap-desktop` (it needs FUSE 2, or run it with
`--appimage-extract-and-run`). On macOS take the `.dmg`; the scripts stop there rather than fetch a file that
would not run. What is checked is that the installer is the file `SHA256SUMS` of this release lists: that proves
it is whole and the one of this release, not who made the release.

## Files

| System | Installer |
| ------ | --------- |
| Windows (x86-64) | `docmap-desktop_<version>_x64-setup.exe` (NSIS, per user) and `docmap-desktop_<version>_x64_en-US.msi` |
| Linux (x86-64) | `docmap-desktop_<version>_amd64.AppImage`, `.deb` and `docmap-desktop-<version>-1.x86_64.rpm` |
| macOS | `docmap-desktop_<version>_aarch64.dmg` (Apple silicon) and `docmap-desktop_<version>_x64.dmg` (Intel), plus an `.app.tar.gz` each; **no bundled engine** |

The Windows and Linux installers bundle the engine of `docmap.nvim`. The macOS ones do not: they find an
engine on `PATH` or say they found none.

`SHA256SUMS` lists the checksum of every file (installers and scripts):

```
sha256sum -c SHA256SUMS --ignore-missing
```

The installers are not signed yet: Windows may show SmartScreen for a download made in the browser, macOS may
refuse to open the app until you allow it. The scripts do not need either.
