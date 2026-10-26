# winget manifests — `GKartist75.Wan2GP`

The three YAML files in this folder describe the Windows NSIS installer to
[winget](https://learn.microsoft.com/windows/package-manager/). They are the
copy kept in this repo so the package definition is reviewable and versioned
next to the code that builds the installer.

**They are not what winget reads.** `winget install GKartist75.Wan2GP` resolves
against [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs), under
`manifests/g/GKartist75/Wan2GP/<version>/`. The files here are the shape of that
submission; the live ones are opened as a pull request:

| Lane | How the manifest reaches winget-pkgs |
| --- | --- |
| Manual, first submission only | `wingetcreate new <installer-url> --submit`, or paste the three files here into a fork of winget-pkgs at the path above. **Required once:** the automated lane updates an existing package (it runs Komac's `update`), so it cannot create the identifier. |
| Automated (every release after that) | `.github/workflows/package-managers.yml` runs on `release: published` and `vedantmgoyal2009/winget-releaser` renders the version, URL and SHA-256 from the release's own `*-setup.exe`, then opens the PR. Needs the `WINGET_TOKEN` repository secret (a classic PAT with `public_repo` from the account that owns the winget-pkgs fork). No token → the job skips; nothing fails. |
| Manual (any release) | `wingetcreate update GKartist75.Wan2GP -u <installer-url> -v <version> --submit` (add `--token <pat>` in CI). Same result: a PR, then a moderator merges it. |

Consequences worth knowing:

- `PackageVersion` here is only a reference point. Bumping these files in a
  launcher PR changes nothing for users — the release workflow is the bump.
- The `InstallerSha256: <SHA256>` in `GKartist75.Wan2GP.installer.yaml` is a
  placeholder, so this copy fails `winget validate`. That is intended: the hash
  cannot exist before the installer is built. The workflow supplies it.
- The installer URL uses dots (`Wan2GP.Desktop.Launcher.Tauri_…`), because
  GitHub rewrites the spaces in the bundle's file name on upload — the local
  copy in `src-tauri/target/release/bundle/nsis/` still has spaces.
- winget ships the same NSIS installer the in-app updater uses, so a winget
  upgrade and an in-app upgrade install identical binaries. Do not run both at
  once on the same copy.
- Install/upgrade, once the package is published:

  ```powershell
  winget install  --id GKartist75.Wan2GP -e
  winget upgrade  --id GKartist75.Wan2GP -e
  ```
