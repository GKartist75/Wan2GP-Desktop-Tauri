# Updating the launcher

The launcher updates itself where it can, and tells you the exact next step
where it cannot. Two rules hold everywhere:

- It never runs a package manager for you, and never asks for a password. When
  an update needs `sudo`, the command is **shown** — you copy it into a terminal.
- It never installs something silently. A check runs shortly after launch (8 s,
  once the backend is up) and every 5 h after that
  (`src/updatecheck-tab.js`); a check only lights up the update control and
  announces it once. The download starts when you press the control.

The launcher knows which of six install kinds it is, from its own bundle marker
(`LauncherInstall` in `src-tauri/src/updates.rs`: `appimage`, `deb`, `rpm`,
`windows`, `macapp`, `source`). Every row below is one of them.

## Which lane am I on?

| Install kind | What you installed | How it updates | Exact command when one is needed |
| --- | --- | --- | --- |
| `windows` | `*-setup.exe` (NSIS) or `.msi` | **In-app**, in place: download → *install & restart* | none — press the button |
| `appimage` | `*.AppImage` | **In-app** once the release carries a Linux entry in `latest.json`; otherwise replace the file | `chmod +x Wan2GP-Desktop-Launcher_<version>_amd64.AppImage` |
| `deb` | `sudo apt install ./<file>.deb` | **Download, then install by hand** | `sudo apt install ./Wan2GP-Desktop-Launcher_<version>_amd64.deb` |
| `rpm` | `sudo rpm -Uvh <file>.rpm` | **Download, then install by hand** | `sudo rpm -Uvh ./<file>.rpm` |
| `macapp` | `.dmg` → dragged into *Applications* | **Download, then drag** | none — open the `.dmg`, drag, replace |
| `source` | `cargo build --manifest-path src-tauri/Cargo.toml` | **Rebuild** | `git pull` then `cargo build --manifest-path src-tauri/Cargo.toml` |

Release asset names (GitHub rewrites the spaces in the bundle's file name to
dots, which is why these are dotted while the local build output is not):

| Platform | Assets |
| --- | --- |
| Windows x64 | `Wan2GP-Desktop-Launcher_<version>_x64-setup.exe` (NSIS), plus the equivalent `.msi` |
| Linux x86_64 | `Wan2GP-Desktop-Launcher_<version>_amd64.AppImage`, `…_amd64.deb` |
| macOS arm64 | `Wan2GP-Desktop-Launcher_<version>_aarch64.dmg` — no updater payload, by design |

One merged `latest.json` on the release is what `tauri-plugin-updater` reads;
each platform leg appends its own entry rather than overwriting the file.

## Windows — NSIS / MSI (`windows`)

The only fully turnkey path, and the one in use today.

1. **Dashboard → Check Desktop Updates** (or **Manage → Updates**).
2. If a newer version exists the button becomes *Update available — vX*; press
   it to download.
3. When the download lands the button becomes *vX ready — click to install*;
   pressing it swaps the installer in **passive** mode (`installMode: passive`
   in `src-tauri/tauri.conf.json`), so you see a progress bar and the app exits.
4. Start the launcher again. Your install folder, models and settings are not
   touched — the installer only replaces the launcher's own files.

An MSI install updates through the same in-app flow: `latest.json` carries one
Windows entry, and the plugin runs whatever installer that entry points at —
the same `-setup.exe` a winget upgrade would fetch.

## Linux — AppImage (`appimage`)

Self-update works **once the release carries a `linux-x86_64` entry in
`latest.json`**, which requires `TAURI_SIGNING_PRIVATE_KEY` (and its password)
in the repository secrets when `release.yml` runs. Without a signed entry the
launcher cannot verify the payload, so it offers the download instead of
swapping the file.

Keep the `.AppImage` in a stable place (e.g. `~/Applications`) — the update
replaces *that file*, and it has no way to find a copy you renamed and moved.

In-app: download → *install & restart*. The plugin swaps the file, then the
button reads *vX installed — click to restart* (an AppImage cannot restart the
running process by itself).

By hand: download the new `…_amd64.AppImage`, replace the old one, then

```bash
chmod +x Wan2GP-Desktop-Launcher_<version>_amd64.AppImage
./Wan2GP-Desktop-Launcher_<version>_amd64.AppImage
```

The AppImage needs FUSE to mount (`sudo apt install libfuse2t64` on Ubuntu
24.04+/26.04, `libfuse2` before that). Without FUSE, run it with
`--appimage-extract-and-run`.

## Linux — `.deb` and `.rpm` (`deb`, `rpm`)

Package-managed installs are **download-then-install by hand**. The launcher
will not run `apt`/`rpm` or prompt for a password; it hands you the command with
the real asset name already in it.

```bash
# Debian / Ubuntu
sudo apt install ./Wan2GP-Desktop-Launcher_<version>_amd64.deb

# Fedora / RHEL / openSUSE
sudo rpm -Uvh ./<the-downloaded>.rpm
```

`.deb` installs `/usr/bin/wan2gp-desktop-launcher-tauri` plus a menu entry, and
declares `libwebkit2gtk-4.1-0` + `libgtk-3-0`. `release.yml` builds AppImage and
`.deb` today; the `rpm` kind exists for distro/maintainer builds
(`--bundles rpm`), and a launcher built that way reports itself correctly.

Run the upgrade with the launcher closed, then start it again. Upgrading the
*launcher* leaves your WanGP install and models alone — that is a separate
update (**Manage → Updates → Update WanGP**).

## macOS — `.dmg` (`macapp`)

Open the `.dmg`, drag **Wan2GP Desktop Launcher Tauri** into *Applications*, and
confirm *Replace* when macOS asks. Unsigned and not notarized, so the first
launch after an update may need right-click → **Open**, or:

```bash
xattr -dr com.apple.quarantine "/Applications/Wan2GP Desktop Launcher Tauri.app"
```

**In-app macOS self-update is not enabled.** Replacing a `.app` in place requires
the new bundle to be signed with a **Developer ID** certificate and notarized by
Apple; without that, `tauri-plugin-updater` would either refuse the payload or
leave an app macOS quarantines. `release.yml` therefore forces
`createUpdaterArtifacts: false` on the macOS leg, and the launcher's macOS
controls are download links, not install buttons. This is a signing-identity
gap, not a missing code path: add an Apple Developer ID + notarization to the
release workflow and the same in-app flow the Windows build uses becomes
available.

Apple Silicon only — `release.yml` builds `aarch64-apple-darwin`; there is no
Intel `.dmg`.

## Source builds (`source`)

When the launcher runs from a `cargo build` output (or an unpacked binary) there
is no bundle for the updater to patch, and on Linux the plugin would happily
treat the running executable as an AppImage and move the build aside. So the
launcher refuses to self-update in this mode: the update control reads
**Releases** and opens the releases page instead of downloading anything, and
**Manage → Updates** shows *this copy runs from a build directory — git pull,
then rebuild*.

```bash
git pull
cargo build --manifest-path src-tauri/Cargo.toml
# release build, with bundles:
npm ci && npm run tauri -- build
```

## Package managers — winget and Homebrew

**Available once the manifests are published.** The definitions live in this
repo (`packaging/winget/`, `packaging/homebrew/Casks/wan2gp-desktop.rb`) and
`.github/workflows/package-managers.yml` submits them on every published
release — but winget reads only `microsoft/winget-pkgs`, and brew reads only
`GKartist75/homebrew-tap`. Until those two exist, use the download lanes above.

```powershell
# Windows — once GKartist75.Wan2GP is in winget-pkgs
winget install --id GKartist75.Wan2GP -e
winget upgrade --id GKartist75.Wan2GP -e
```

```bash
# macOS arm64 — once the cask is in GKartist75/homebrew-tap
brew install --cask GKartist75/tap/wan2gp-desktop
brew upgrade --cask wan2gp-desktop
```

Both lanes install the *same* artifacts as the manual download (winget: the
NSIS `-setup.exe`; brew: the arm64 `.dmg`). A package manager only knows the
version its manifest was published with, and the in-app updater does not wait
for a manifest PR to merge — so pick one lane per machine and let it own the
updates.

## What the launcher tells you

The update control is a state machine (`src/autoupdate-tab.js`); the same text
appears on the dashboard button and in **Manage → Updates**.

| State | What it says | What pressing does |
| --- | --- | --- |
| Checking | *Checking…* | nothing (a check is already running) |
| Up to date | *Up to date ✓* | runs a fresh check |
| Update available | *Update available — vX* (toast: *click to download*) | downloads the update |
| Downloading | *Downloading vX — NN%* | nothing |
| Downloaded | *vX ready — click to install* | installs and restarts (Windows: passive installer; AppImage: swaps the file) |
| Installed, restart needed | *vX installed — click to restart* (Linux AppImage) | restarts the launcher |
| Not self-updatable (`deb`, `rpm`, `macapp`, or no asset for this platform) | button *vX — Download*; **Manage → Updates**: *vX available — what to do (asset name)* | opens the download in your browser |
| Not self-updatable, source build | button *vX — Releases*; **Manage → Updates**: *this copy runs from a build directory — git pull, then rebuild* | opens the releases page |
| Error | *Error: …* | retries the check |

"Not self-updatable" is the honest case, and it is the reason this page exists:
a `.deb`, an `.rpm`, a `.dmg`, a source build, or a release with no asset for
your platform all land there. The wording comes from
`src/services/launcher-update.js` — it names the artifact you need, and for
`deb`/`rpm` **Manage → Updates** shows the finished `sudo …` line with the
actual file name in it, ready to copy.
