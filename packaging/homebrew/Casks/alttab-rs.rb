cask "alttab-rs" do
  version "0.1.0"
  sha256 "REPLACE_WITH_SHA256"

  url "https://github.com/jimbo23/alttab-rs/releases/download/v#{version}/AltTab-rs-#{version}.zip"
  name "AltTab-rs"
  desc "Native Rust window switcher for macOS, derived from AltTab"
  homepage "https://github.com/jimbo23/alttab-rs"

  depends_on macos: ">= :monterey"

  app "AltTab-rs.app"

  zap trash: [
    "~/Library/Application Support/alttab-rs",
    "~/Library/Caches/io.github.jimbo23.alttab-rs",
    "~/Library/Preferences/io.github.jimbo23.alttab-rs.plist",
    "~/Library/Saved Application State/io.github.jimbo23.alttab-rs.savedState",
  ]

  caveats <<~EOS
    Grant Accessibility access in System Settings to enable global switching.
    Screen Recording access enables thumbnails; you may continue without it.
    AltTab-rs is ad-hoc signed, not Developer ID signed or notarized. Gatekeeper
    may block it. Only for a release you trust, install using:
      brew install --cask --no-quarantine jimbo23/alttab-rs/alttab-rs
    Or remove quarantine after installation:
      xattr -dr com.apple.quarantine /Applications/AltTab-rs.app
    AltTab-rs is a GPL-3.0-only derivative of AltTab by lwouis and contributors.
  EOS
end
