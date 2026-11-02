# AltTab-rs

A native Rust macOS window switcher derived from [AltTab](https://github.com/lwouis/alt-tab-macos), by **lwouis and the AltTab contributors**. This is a derivative work of GPL-3.0 code and is distributed under the same license, **GPL-3.0-only**; see [LICENSE](LICENSE) and [attribution](docs/ATTRIBUTION.md). Not affiliated with or endorsed by the upstream project.

Hold **Option**, press **Tab** to select another window, then release Option to focus it. Shift-Tab reverses direction; arrows navigate; Escape cancels. Preferences offer Command instead of Option. The application uses a menu-bar item rather than a Dock icon.

## Install

Requires macOS Monterey (12.0) or newer, Intel or Apple Silicon. Release bundles are universal and ad-hoc signed, **not Developer-ID signed or notarized**. Releases and the tap must first be published by the maintainer; the commands below do not imply they already exist.

```sh
brew install --cask jimbo23/alttab-rs/alttab-rs
# Equivalent:
brew tap jimbo23/alttab-rs
brew install --cask alttab-rs
```

If Gatekeeper blocks the unsigned application, inspect and trust its source before deliberately bypassing quarantine:

```sh
brew install --cask --no-quarantine jimbo23/alttab-rs/alttab-rs
# Or, after installing the app:
xattr -dr com.apple.quarantine /Applications/AltTab-rs.app
```

These commands remove a security check; they do not notarize the app. Do not disable Gatekeeper globally.

### Permissions

Grant **Accessibility** in System Settings → Privacy & Security for global shortcut interception and window focus. Grant **Screen Recording** for window thumbnails. Without recording access, icon/title switching remains available. The application offers permission prompts and links to the relevant settings panes. Permission changes may require relaunching the application on some macOS versions. Ad-hoc rebuilds can affect TCC identity; keep the bundle identifier stable.

## Design and feature boundary

Portable policies live in `alttab-core`; AppKit and Apple-framework ownership live in `alttab-macos`. [SPEC](docs/SPEC.md) records source-backed upstream behavior; [ARCHITECTURE](docs/ARCHITECTURE.md) records intended policies and macOS validation obligations. Upstream is reference-only, not a build dependency.

Memory policy: capture only for a visible thumbnail switcher, downscale to display dimensions, bound retained thumbnails with a default **32 MiB LRU**, and release screenshot image ownership on hide. No screenshot disk cache or continuous screen stream. The cache budget is **not** an RSS limit: framework mappings, compositor copies, transient capture buffers, and allocator retention are separate costs. **Idle RSS below 20 MiB is an unmeasured target, not a demonstrated result.** Native memory and responsiveness need measurement on macOS.

| Feature versus upstream | Coverage |
|---|---|
| Window model, MRU, composable filtering | Done in portable core; native discovery needs macOS validation |
| Option/Command-Tab, reverse, arrows, release-to-focus, Escape | Done; native delivery needs macOS validation |
| Thumbnail, icon, and title presentations | Done; native rendering needs macOS validation |
| Minimized, hidden, fullscreen preferences | Done; AX behavior needs macOS validation |
| Multiple displays and Spaces | Partial: geometry/display filtering and optional runtime-resolved SkyLight membership; not upstream WindowServer parity |
| Exact-window focus across applications/Spaces | Partial: public activation + AX raise is weaker than upstream private focus sequencing |
| Permissions and login registration | Done; TCC and SMAppService require macOS validation |
| Native tab/sheet reconciliation, remote AX token acquisition | Partial; not upstream-equivalent |
| Search and advanced preferences | Partial: search and a validated native TOML editor, not upstream fuzzy matching/IME behavior or dedicated controls for every enum |
| Close/minimize/fullscreen window, hide/quit app shortcuts | Done via public AX/NSRunningApplication; native behavior unverified |
| Gestures | Not planned |
| Upstream Pro/licensing, Sparkle, telemetry, crash reporting, translations | Not planned |

“Done” describes implemented portable behavior, not proof of native OS integration. No macOS runtime observation was possible on the Linux implementation host.

## Configuration

`~/Library/Application Support/alttab-rs/config.toml` contains the TOML configuration. Missing files use defaults; malformed configuration is an error, not an instruction to overwrite the original. Core persistence accepts an explicit path for testing. Preferences expose essential native controls and a validated TOML editor for advanced policy. Quit before editing the file externally, then relaunch.

The portable cache also exposes an opt-out retention policy,
`capture.evict_on_hide = false`, for consumers that deliberately retain ready
entries. Pending captures are invalidated regardless. The native app rejects
this opt-out because its stronger memory contract requires zero retained
screenshots after hide.

## Build from source

Install stable Rust and the two Apple targets. Building the actual app requires macOS command-line tools and the Apple SDK; do not use an unofficial SDK download.

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
# On macOS, build a universal, ad-hoc-signed application and release zip:
./scripts/bundle.sh
```

The bundle script creates `target/AltTab-rs.app` and `dist/AltTab-rs-<version>.zip`, then prints its SHA-256. The icon is the existing `assets/icon/AppIcon.icns`. [Homebrew tap setup](packaging/homebrew/README.md) describes publication; local development does not create repositories or push anything.

### Linux verification boundary

The portable core and non-macOS message stub run on Linux. Apple bindings can be type-checked without an Apple linker:

```sh
cargo check --target aarch64-apple-darwin -p alttab-macos
cargo clippy --target aarch64-apple-darwin -p alttab-macos -- -D warnings
cargo check --target x86_64-apple-darwin -p alttab-macos
cargo clippy --target x86_64-apple-darwin -p alttab-macos -- -D warnings
```

Portable validation on the Linux implementation host used Rust 1.99.1:
37 behavior tests passed, strict workspace Clippy passed, and a temporary
smoke program exercised filtering → layout → release-to-focus → cache teardown
(three candidates, three tiles, zero hidden thumbnail bytes). The Linux binary
printed its unsupported-platform message. The smoke program was then removed.

Both `cargo check` and strict Clippy passed for the native crate on
`aarch64-apple-darwin` and `x86_64-apple-darwin`. Formatting passed across the
workspace. Packaging checks parsed both workflow YAML files and Info.plist,
checked shell syntax, and exercised the bundle script's Linux rejection.

The native adapter uses one serialized observation/query lane plus separate
command, capture, and icon workers. It does not yet provide the design's
two-query-lane per-process isolation: an unresponsive app can delay discovery
of other apps, though AX messaging timeouts bound individual calls. Full
upstream observer-health recovery, remote AX acquisition, native-tab
reconstruction, and child-surface capture expansion are not reproduced.

A Darwin `cargo build` on Linux is expected to fail at link time without Apple tools. Cross-checking does not verify framework loading, selector availability on older systems, permission prompts, keyboard taps, panel focus/Spaces behavior, captures, login registration, signing, or memory usage. Those require the macOS CI build and real interactive testing on supported Intel/Apple Silicon OS versions. See the [native acceptance checklist](docs/ARCHITECTURE.md#verification-plan-and-unresolved-risks).

## Project layout

- `crates/alttab-core`: window identities, MRU/filter policies, selection, geometry, TOML configuration, and cache accounting; only serde/TOML dependencies.
- `crates/alttab-macos`: native app, event input, tracking, capture, focus, preferences, and permissions; Apple dependencies are target-gated.
- `docs/`: behavioral evidence, architecture, attribution.
- `macos/Info.plist`, `scripts/bundle.sh`: application identity and universal packaging.
- `packaging/homebrew/`: cask template and tap instructions.
- `.github/workflows/`: portable/native CI and release/tap automation.
- `assets/icon/`: supplied SVG, PNG, ICNS, and icon generation source.
