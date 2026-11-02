You are working in /workspace/alttab-rs (empty local git repo, LICENSE = GPL-3.0 copied from upstream). The upstream reference source is /workspace/alttab-src (lwouis/alt-tab-macos, commit c500294bdb01186011a5c64a412b04cef9931bef, GPL-3.0, Swift). Treat /workspace/alttab-src as READ-ONLY.

Use the installed `reverse-engineer-anything` skill (from github.com/morluto/rea) as the investigation method: evidence-backed claims with file:line citations, separate Observations from Inferences, record Unknowns/limitations. Because complete upstream source is available, per the skill you should use normal repository tools (rg, reading files) rather than REA native-binary providers; you may use REA MCP tools only if they add value.

TASK (phase 1: spec + architecture only, no app implementation yet):
1. Write docs/SPEC.md: a feature spec of AltTab reverse-engineered from the source, with citations (path:line into alttab-src). Cover at minimum:
   - Global shortcut handling (hold modifier Alt/Option or Cmd + Tab, Tab/Shift-Tab/arrow cycling, release-to-focus, Esc cancel, shortcut variants), and how upstream intercepts events (CGEventTap / Carbon hotkeys).
   - Window enumeration & tracking (AXUIElement observers, CGWindowList, private SkyLight/CGS APIs, Spaces, minimized/hidden windows, app launch/quit), ordering (MRU / z-order).
   - Thumbnails (ScreenCaptureKit vs CGWindowListCreateImage / private APIs), refresh cadence, caching.
   - Focusing a window (AX raise, NSRunningApplication activate, private _SLPSSetFrontProcessWithOptions etc.).
   - Switcher UI layout (grid of thumbnails/app icons/titles, max rows/width ratios, multi-screen placement).
   - Preferences essentials (shortcut choice, appearance style thumbnails/app icons/titles, which windows to show: minimized/hidden/fullscreen/other spaces/other screens, blacklist, launch at login, menubar icon).
   - Permissions (Accessibility, Screen Recording) and onboarding.
   - Explicitly OUT of scope: anything under src/pro (Pro licensing/upsell), Sparkle auto-updates, localization beyond English, telemetry/crash reporting.
2. Write docs/ARCHITECTURE.md for a Rust rewrite named alttab-rs:
   - Cargo workspace: crates/alttab-core (pure Rust, platform-independent, 100% testable on Linux: window model, MRU ordering, filtering rules, selection/cycling state machine, grid layout math, config schema+load/save TOML, thumbnail cache policy/LRU with byte budget), and crates/alttab-macos (binary; objc2 + objc2-app-kit + objc2-foundation + objc2-core-graphics/core-foundation and an AX/ScreenCaptureKit FFI layer; no Electron/webview). Keep dependencies minimal; prefer objc2 family crates. Pin versions you will use (check crates.io index via `cargo search` or `cargo info` where possible).
   - Memory optimization plan with concrete numbers: idle RSS target (e.g. < 20 MB idle), lazy capture only while switcher visible, thumbnail downscale to display size, BGRA->compressed or 8-bit? , LRU byte budget (default 32 MB) with eviction on hide, release of CGImage/IOSurface after hide, no per-frame allocations, autorelease pool discipline, avoid retaining NSImage for hidden windows.
   - Mapping table: upstream component -> Rust module.
3. Write docs/ATTRIBUTION.md crediting AltTab (lwouis, GPL-3.0) and noting alttab-rs is a derivative work under GPL-3.0.
4. Commit with git (local only). Do NOT create GitHub repos, do NOT push, do NOT post anywhere.

Be thorough but keep SPEC.md focused (aim ~300-600 lines). End your reply with a short summary.
