# Installing Akshara Office

Akshara never needs administrator rights or an internet connection. Every
package installs for the current user only.

## Linux

```bash
tar xzf akshara-0.1.5-linux-x64.tar.gz
cd akshara-0.1.5-linux-x64
./install.sh
```

This copies the app to `~/.local/share/akshara`, adds the `akshara` command
to `~/.local/bin`, and adds a menu entry plus file associations for `.odt`,
`.ods`, `.odf` and `.csv` (in `~/.local/share/applications`). Remove it with
`./uninstall.sh` (`--purge` also deletes settings in `~/.config/akshara`).
You can also run `./bundle/akshara` without installing.

Requirements: 64-bit Linux with GTK 3 and OpenGL ES/EGL (standard on
desktop distributions).

## Windows

Run `akshara-0.1.5-windows-x64-setup.exe`. It installs into
`%LOCALAPPDATA%\Programs\Akshara` without a UAC prompt, creates Start menu
and (optionally) desktop shortcuts, and can register Akshara for OpenDocument
files for your account only. Uninstall it from *Settings → Apps*.

Alternatively, unzip `akshara-0.1.5-windows-x64-portable.zip` anywhere and run
`akshara.exe`.

## macOS

Open `akshara-0.1.5-macos.dmg` and either double-click **Install for me**
(it copies the app to `~/Applications`), or drag **Akshara.app** to the
`Applications` folder in your home directory. Builds signed ad hoc must be
opened once with right-click → *Open*.

The macOS app is sandboxed. Its file browser starts in the app's own
Documents folder (inside `~/Library/Containers/org.akshara.office`). Opening
files elsewhere through the system file panel is planned.

## Android

Copy the APK for your device (`app-arm64-v8a-release.apk` for almost all
current phones) and open it. Allow *Install unknown apps* for your file
manager when asked; no root is needed. Documents are stored in the app's
storage. The *Shared (Android/data)* folder in the file browser
(`Android/data/org.akshara.office/files`) can be reached from a computer over
USB to exchange files.

The release app requests **no permissions at all**, not even Internet.

## iOS

iOS apps must be signed by Apple. Install through TestFlight or the App
Store, or build and sign it yourself with Xcode (see `docs/building.md`).
Documents live in the app's Documents folder, which appears in the Files app
and in Finder (file sharing is enabled).

## Settings and data

| Platform | Settings |
|---|---|
| Linux | `~/.config/akshara/settings.json` |
| Windows | `%APPDATA%\Akshara\settings.json` |
| macOS | `~/Library/Application Support/Akshara/` (inside the sandbox container) |
| Android / iOS | inside the app's private storage |

The settings file contains only the interface language, the theme, the look
and feel, the transliteration preferences and the recent-files list.
