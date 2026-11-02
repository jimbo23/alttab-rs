# Homebrew distribution

The separate tap is `jimbo23/homebrew-alttab-rs`. This directory contains its
`Casks/alttab-rs.rb` template; the checked-in `REPLACE_WITH_SHA256` is deliberately
not installable. A release fills the version and SHA-256 from the actual archive.

## Create the tap

1. Manually create a public GitHub repository named
   `jimbo23/homebrew-alttab-rs`, initialized with a README and a default branch.
   Repository creation is not performed by this project's scripts.
2. Copy `Casks/alttab-rs.rb` from this directory to `Casks/alttab-rs.rb` in that
   repository. Before publishing it for installation, replace the placeholder
   with the SHA-256 printed by `scripts/bundle.sh` for the corresponding release.
3. Create a fine-grained GitHub token restricted to the tap repository with
   **Contents: read and write**, and store it as the `HOMEBREW_TAP_TOKEN` Actions
   secret in `jimbo23/alttab-rs`. Organization policies may require approval.
   The main repository's `GITHUB_TOKEN` generally cannot push to another repo.
4. Release a Cargo version by pushing its matching `v<version>` tag, or run the
   Release workflow manually. The workflow builds both Darwin architectures,
   ad-hoc signs the universal app, archives it, and uploads the zip to the
   `v<version>` GitHub Release. Only after successful upload, if the token is
   nonempty, it checks out the tap, fills the cask from this template, commits
   changes, and pushes **only to the tap**. Without the secret, publication
   still succeeds and no tap is checked out or changed.

Keep the Cargo package version authoritative. Tag builds reject a tag/version
mismatch. Manual runs publish the current package version; avoid rerunning an
already-distributed version with different binaries. If a tap update fails after
upload, correct its access/branch policy and rerun the workflow or update the
cask manually using the released zip's checksum.

## Install

```sh
brew install --cask jimbo23/alttab-rs/alttab-rs
# Equivalent:
brew tap jimbo23/alttab-rs
brew install --cask alttab-rs
```

macOS Monterey (12.0) or newer is required; the app is universal Intel/Apple
Silicon. Releases are ad-hoc signed, not Developer ID signed or notarized.
Gatekeeper can refuse to open a quarantined download. For an artifact you have
verified and trust, use
`brew install --cask --no-quarantine jimbo23/alttab-rs/alttab-rs`, or run
`xattr -dr com.apple.quarantine /Applications/AltTab-rs.app` after installation.
These commands bypass quarantine checks; they are not signing or notarization.

Grant Accessibility access in System Settings for global switching. Screen
Recording enables window thumbnails and can be skipped for icon/title switching.
`brew uninstall --cask --zap alttab-rs` also removes configuration, preferences,
cache, and saved window state listed in the cask; ordinary uninstall preserves
those files. macOS permission grants are managed by the OS, not removed by zap.

AltTab-rs is GPL-3.0-only and derives from
[AltTab by lwouis and contributors](https://github.com/lwouis/alt-tab-macos).
The application bundle includes the license and upstream attribution. Neither
the icon nor upstream's application/signing identity is replaced by packaging.

## Verification boundary

The script requires macOS command-line tools (`lipo`, `codesign`, `ditto`, and
`PlistBuddy`) and Rust with both Darwin targets installed. CI supplies the Rust
targets; local builders must install them first. Linux cannot verify linking,
universal loading, signatures, Gatekeeper, permission prompts, or application
launch. GitHub release upload and optional tap authentication require a real
workflow run; no publishing commands are executed during implementation.
