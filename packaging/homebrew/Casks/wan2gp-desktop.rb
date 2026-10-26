# Homebrew cask — the arm64 .dmg release asset.
#
# The copy users install from lives in the tap, not here:
#   GKartist75/homebrew-tap/Casks/wan2gp-desktop.rb
# This file is the source of truth for its shape; `.github/workflows/package-managers.yml`
# bumps the tap copy on every published release with `brew bump-cask-pr`, which
# supplies the version and pins the real sha256 (hence `sha256 :no_check` below,
# the state a brand-new cask starts in). No tap → users install by hand from the
# releases page, which docs/UPDATING.md covers.
#
# `app` is the bundle's `productName` from src-tauri/tauri.conf.json, verbatim —
# a wrong name makes `brew install` fail at the link step, after the download.
cask "wan2gp-desktop" do
  version "0.10.25"
  # Placeholder for a cask before its first bump: the digest can only be
  # computed after the .dmg is published. `brew bump-cask-pr --sha256 …` replaces
  # it in the tap.
  sha256 :no_check

  url "https://github.com/GKartist75/Wan2GP-Desktop-Tauri/releases/download/v#{version}/Wan2GP.Desktop.Launcher.Tauri_#{version}_aarch64.dmg"
  name "Wan2GP Desktop Launcher"
  desc "One-click desktop launcher for the WanGP generative AI toolkit"
  homepage "https://github.com/GKartist75/Wan2GP-Desktop-Tauri"

  # Hits the releases API itself rather than the HTML page: `tag_name` is the
  # only value livecheck needs, and it matches the `v<version>` release
  # directory the .dmg is published under.
  livecheck do
    url "https://api.github.com/repos/GKartist75/Wan2GP-Desktop-Tauri/releases/latest"
    strategy :json do |json|
      json["tag_name"]&.delete_prefix("v")
    end
  end

  # bundle.macOS.minimumSystemVersion is 11.0 and release.yml only builds
  # aarch64-apple-darwin — there is no Intel .dmg to fall back to.
  depends_on macos: ">= :big_sur"
  depends_on arch: :arm64

  app "Wan2GP Desktop Launcher Tauri.app"

  # The launcher's own state: the data-dir override recorded by the folder
  # picker, the "this install was performed" marker, and WKWebView's default
  # data store (renderer localStorage: collapsed cards, theme, last save dir).
  #
  # Deliberately NOT zapped: the data dir itself (default `~/Wan2GP`) and its
  # `-Models` sibling. They hold the WanGP checkout, the Python env and every
  # downloaded model — tens of GB of user data that an uninstall must not take
  # with it. Remove them by hand if that is really what you want.
  zap trash: [
    "~/.wan2gp-tauri-data-dir",
    "~/.wan2gp-tauri-installed",
    "~/Library/WebKit/com.wangp.desktop-tauri",
  ]
end
