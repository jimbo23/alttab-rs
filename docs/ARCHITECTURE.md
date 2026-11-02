# alttab-rs architecture

## Status and evidence boundary

The phase-1 design below is the behavioral and ownership contract for the Rust
implementation. [SPEC.md](SPEC.md) records upstream observations and source
citations; [ATTRIBUTION.md](ATTRIBUTION.md) records the GPL derivative relationship.
The implementation and its verified platform boundary are described in the root
README. Design targets below are not measured performance results.

**Observation:** upstream isolates AX, WindowServer, and process IPC in separate
bounded schedulers (`src/macos/AXCallScheduler.swift:3-31,61-95`,
`src/macos/CGSCallScheduler.swift:3-30`,
`src/macos/ProcessCallScheduler.swift:3-21`). These paths can block despite being
ordinary SDK calls. Its main-thread audit distinguishes AppKit responder/view
operations from thread-safe NSWorkspace/NSRunningApplication operations
(`src/main-thread-ipc.md:17-32,62-90`). Those are upstream recorded findings,
not measurements made during this investigation.

**Inference:** the highest-value boundary is deterministic policy versus OS
ownership and IPC, rather than one Rust crate per upstream Swift class.

## Workspace and platform boundary

**Decision:** create this workspace in the implementation phase:

```text
Cargo.toml                       # resolver = "2", edition = "2021"
Cargo.lock                       # committed for reproducible application builds
crates/
  alttab-core/
    Cargo.toml
    src/
      lib.rs
      model.rs                   # stable IDs, window/app/display/Space snapshots
      order.rs                   # MRU ledger and alternate stable orders
      filter.rs                  # profile predicates and blacklist rules
      session.rs                 # selection, cycling, release/cancel transitions
      layout.rs                  # grid/list geometry, row navigation, hit testing
      config.rs                  # versioned TOML schema, validation and persistence
      thumbnail.rs               # byte accounting, LRU, capture eligibility
  alttab-macos/
    Cargo.toml
    src/
      main.rs                    # native NSApplication and startup
      app.rs                     # coordinator and effect dispatch
      input.rs                   # Carbon hotkeys + CGEvent taps
      tracking.rs                # NSWorkspace and AX notification reconciliation
      spaces.rs                  # topology and display membership
      focus.rs                   # activation and exact-window focus
      capture.rs                 # screenshot requests and owned image resources
      permissions.rs             # AX/TCC status and onboarding
      settings.rs                # native preferences UI and login registration
      ui.rs                      # AppKit panel, reusable tiles, status item
      ffi/
        ax.rs                    # ApplicationServices ownership and callbacks
        carbon.rs                # hotkey registration and process records
        skylight.rs              # optional dynamically resolved private symbols
        screen_capture.rs        # checked ScreenCaptureKit wrapper
        service_management.rs    # SMAppService bridge
```

`alttab-core` uses only Rust/std plus serde and TOML. No Objective-C types,
CF/CG handles, platform clocks, display queries, or hidden global state.
Every public policy and error path is exercisable on Linux; this means complete
platform-independent testability, not a claim of measured 100% test coverage.
Filesystem persistence accepts an explicit path and is tested in temporary
Linux directories. macOS supplies the application-support directory.

The binary depends on core; core never depends on the binary. Workspace
`default-members` is core. Apple dependencies remain under
`cfg(target_os = "macos")`; explicitly running the binary on another platform
prints an unsupported-platform message.
No Electron, webview, SwiftUI, Interface Builder, Tokio runtime, database,
image-codec stack, or general-purpose GUI framework.

The implementation uses edition 2021; no minimum Rust version is promised beyond
the toolchain verified in the README. Deployment targets macOS 12.0 on arm64 and
x86_64. Capture uses SCScreenshotManager on macOS 14+ and a downscaled
CGWindowListCreateImage fallback on older systems. Private-API failure must not
prevent icon/title switching for publicly discoverable windows.

## Dependency pins and provenance

**Observation:** `cargo info <name>` succeeded against crates.io for every row
below on 2026-11-03. These are selected exact package versions, not guesses from
crate naming. Published metadata reports Rust 1.71 for the objc2 family and
helpers, 1.56 for serde, and 1.85 for TOML.

| Direct dependency | Exact Cargo requirement | Role | Version evidence |
|---|---|---|---|
| serde | `=1.0.229` | Core config derives; `derive`, `std` | [crates.io](https://crates.io/crates/serde/1.0.229) |
| toml | `=1.1.8` | Core TOML parse/serialize | [1.1.8+spec-1.1.0](https://crates.io/crates/toml/1.1.8+spec-1.1.0) |
| objc2 | `=0.6.5` | Runtime, retained ownership, main-thread marker | [crates.io](https://crates.io/crates/objc2/0.6.5) |
| objc2-app-kit | `=0.3.2` | NSApplication, NSPanel, native controls | [crates.io](https://crates.io/crates/objc2-app-kit/0.3.2) |
| objc2-foundation | `=0.3.2` | NSObject, strings, collections, notifications | [crates.io](https://crates.io/crates/objc2-foundation/0.3.2) |
| objc2-core-graphics | `=0.3.2` | Event taps, window metadata, CGImage | [crates.io](https://crates.io/crates/objc2-core-graphics/0.3.2) |
| objc2-core-foundation | `=0.3.2` | CF run loops and retained CF values | [crates.io](https://crates.io/crates/objc2-core-foundation/0.3.2) |
| objc2-screen-capture-kit | `=0.3.2` | Shareable windows and screenshot callbacks | [crates.io](https://crates.io/crates/objc2-screen-capture-kit/0.3.2) |
| objc2-quartz-core | `=0.3.2` | Reusable CALayer contents and transactions | [crates.io](https://crates.io/crates/objc2-quartz-core/0.3.2) |
| block2 | `=0.6.2` | Owned asynchronous Objective-C blocks | [crates.io](https://crates.io/crates/block2/0.6.2) |
| dispatch2 | `=0.3.1` | Main queue and bounded GCD scheduling | [crates.io](https://crates.io/crates/dispatch2/0.3.1) |

TOML's published build metadata is `+spec-1.1.0`; Cargo ignores build metadata
in version requirements. The lockfile must record the resolved full version.
Use `default-features = false` for framework crates and explicitly select used
classes/functions. AppKit's default set enables 307 features; Foundation's 177.
ScreenCaptureKit defaults include recording, AVFoundation, and sharing-picker
support that this screenshot-only design does not need.

Start its feature selection with `std`, `SCError`, `SCShareableContent`,
`SCScreenshotManager`, `SCStream` (configuration/filter declarations), `block2`,
`objc2-core-foundation`, and `objc2-core-graphics`; enable any additional typed
signature dependencies only as required by the pinned generated bindings.
Do not invent feature names from framework class names. Review cargo tree on
macOS before accepting the minimal feature closure.

The downloaded AppKit and ScreenCaptureKit manifests accept objc2
`>=0.6.2, <0.8.0` and block2 `>=0.6.1, <0.8.0`; ScreenCaptureKit accepts dispatch2
`>=0.3.0, <0.5.0` and the framework bindings at `0.3.2`. The selected pins satisfy
those declared ranges. This is metadata compatibility, not proof of compilation.
AX, Carbon, ServiceManagement, and private SkyLight calls get narrowly scoped
local FFI wrappers; do not add the older cocoa/core-foundation wrapper families
alongside objc2. Transitive packages will be fixed by Cargo.lock, not duplicated
as arbitrary direct dependencies.

## Core model, identity, and ordering

**Decision:** distinguish `AppKey { pid, incarnation }`,
`WindowKey { app, cg_window_id, incarnation }`, `SpaceId(u64)`, and `DisplayId`.
A PID or CGWindowID alone is not a durable identity. The coordinator allocates
incarnations on observed destruction/recreation and rejects stale callbacks.
Native AX handles live only in the adapter's identity-indexed registry.

A window record contains title, app key, role/classification, geometry in global
points, display/Space membership, minimized/hidden/fullscreen state, eligibility,
and acquisition status. Model `Unknown`, `Known(false)`, and `Known(true)` where
absence of evidence matters; an AX timeout is not window destruction.
App records hold bundle ID, display name, activation policy, and lifecycle.
Windowless-app placeholders are a separate item kind, never CGWindowID zero.
Keep physical surface records separate from admitted destinations: child surfaces
can affect capture bounds without becoming switcher items. AX-root replacement
does not end a window incarnation unless independent lifecycle evidence confirms
destruction. Model tab representatives and windowless placeholders explicitly.

The order ledger stores monotonic focus epochs, not wall-clock timestamps.
Accept normalized attention/focus events; creation and background title updates
do not fabricate user attention. Seed unknown history using observed z-order,
with deterministic tie breakers. Preserve within-app order for an activation
that brings several windows forward; reconcile explicit clicked-window evidence
without treating every sibling AX notification as a separate user switch.

On summon: snapshot a filtered, ordered vector of keys. For MRU, initially select
the second displayed candidate unless the attended item was filtered out, then
the first; handle a sole candidate directly. Default selection may settle as
initial inventory arrives until the user navigates. Once committed by navigation,
selection follows identity, not index. Removing that target selects a recorded
action heir or nearest remaining slot; an empty list dismisses without focus.
Session-local traversal never changes MRU; observed focus does.

Filter predicates compose profile app scope, Space scope, display scope,
minimized/hidden/fullscreen policies, windowless-app policy, and blacklist.
Minimized/hidden/windowless policies use `show`, `hide`, `show_at_end`;
fullscreen uses `show` or `hide`. End buckets follow windowless → hidden →
minimized precedence. Preserve the active-app exception override and literal
title-pattern rules from SPEC. Keep hide-from-list and suppress-shortcuts rules
independent, including fullscreen-only suppression.
Keep unknown membership conservative: preserve records, distinguish them in the
snapshot, and reconcile before claiming exact current-Space filtering.

## Session and effects contract

**Decision:** core owns `Idle`, `PendingShow`, and `Visible` states, carrying
session generation, invoking profile, held-modifier mask, stable selection,
visible order, and delay deadline. Clocks and timers are supplied as inputs.
The macOS coordinator converts raw events to normalized input once.

Inputs include summon, forward/backward cycle, row navigation, modifier change,
explicit accept, cancel, model update, permission change, and timer expiration.
Outputs are small effects: render changes, schedule/cancel delay, capture demand,
hide, and focus target. A release before the show delay can still accept the
selected target without flashing a panel. A duplicate key-up or stale timer is
idempotent. Esc cancels without a focus command. Release behavior is profile
configurable; explicit acceptance is possible without release-to-focus.

Do not infer release from unrelated modifier changes. Core owns the bounded
physical press/release log, modifier matcher, repeat state, and settle-before/
after-event rules; the adapter supplies normalized physical events and clock
readings. Carbon and event-tap delivery must not apply one press twice. Preserve
Shift-only reverse traversal as well as configurable Shift+Tab bindings. Restore
native Cmd+Tab symbolic-hotkey state when relinquishing a conflicting binding.

Switching profiles during a session rebuilds eligibility under that profile and
validates selection. Manual cycling wraps, synthetic repeat stops at boundaries;
row navigation uses horizontal center and the nearest tile on the target row.
Freeze ordering while consuming a single input; render from the same revision.
Only the main coordinator executes effects, so a late capture cannot reactivate
a dismissed session or overwrite images from a newer incarnation.

## Layout and native UI

**Decision:** core takes available screen rect, backing scale, style, size preset,
outer padding, spacing, max width/height ratios, nominal rows, title/icon dimensions,
and thumbnail aspect ratios. It returns panel rect, tile rects, row membership,
scroll extent, and navigation neighbors. Use points for layout; derive pixel
capture sizes by rounding outward with backing scale. Clamp zero/invalid bounds.

Support thumbnail grids, app-icon grids, and title lists, using the observed
presets documented in SPEC. Preserve aspect ratio; fit content rather than
stretching it. Nominal rows determine tile height, not total row count; cap the
viewport and scroll excess rows. Display policy supports pointer screen, attended
window's screen with active-menubar fallback, and primary/menubar screen.
Freeze the chosen screen for a session unless it disappears; then rehome safely.

AppKit owns a nonactivating/focus-capable panel as required by keyboard handling,
reusable native tile views/layers, title labels, selection decoration, a native
preferences window, onboarding, and NSStatusItem. Confirm panel activation and
full-screen/Spaces collection behavior on macOS; Linux layout tests cannot prove
these flags work. No retained screenshot NSImage per tracked window. Layer
contents are attached only for displayed tiles and cleared during dismissal.
App icons are deduplicated per application and separately bounded.

## macOS execution and FFI ownership

**Decision:** keep one model/coordinator owner on the AppKit main thread. All
NSResponder/NSView/NSWindow/NSControl and text-input mutations require
`MainThreadMarker`. Do not use `unsafe impl Send` to bypass AppKit constraints.
SDK-safe NSWorkspace/NSRunningApplication operations may be performed off-main,
with copied plain results returned to the coordinator.

Use an event-tap run-loop thread whose callback performs only input decoding,
constant-time swallow/pass decisions, and enqueueing. Never perform AX reads,
capture, TOML I/O, or AppKit mutation in the tap callback. Re-enable disabled taps
and invalidate held-key state on permission loss or teardown.

Use bounded lanes: two AX query jobs, one AX command job, two WindowServer jobs,
one capture job, and one process/icon job initially. Separate timed-out AX work
from interactive commands; per-process admission prevents one hung application
from monopolizing a lane. Counts are design starting points, not upstream copies
or measured throughput. Keep at most one active plus one latest pending request
per identity/purpose; merge state notifications instead of accumulating closures.
OS timeouts bound synchronous AX calls; a Rust cancellation flag cannot interrupt
an already executing Mach call. Shutdown and permission revocation account for
late completions without retaining UI objects indefinitely.

Use typed callback contexts with clear registration ownership. Drop unregisters
observers/hotkeys/taps before releasing their contexts. No panic may cross an FFI
boundary. RAII wrappers encode Create/Copy ownership versus borrowed Get values;
retain borrowed values only when crossing callback lifetime boundaries.
Callbacks carry keys and generation numbers, not pointers into movable vectors.
All unsafe ABI declarations, private constants, and pointer conversions remain in
`ffi/`; no public core API is unsafe.

Prefer public AX raise and application activation. Put exact-window private
focus sequencing behind a runtime capability check. Resolve SkyLight symbols
with `dlsym`; validate signatures against observed declarations and real macOS
headers/behavior before invocation. A missing symbol disables that capability,
not the whole app. Public fallback may have weaker cross-Space enumeration and
focus guarantees; expose that limitation rather than pretending equivalent
behavior. Never send fabricated private focus event records without verified
layout and target validation.

Dismissal order: mark session closed, stop capture admission, hide/order out the
panel, issue the requested focus effect, then defer noncritical cleanup to a
later main-loop turn. Clear callbacks by generation. A rapid re-summon must not
be hidden or stripped of its layers by old cleanup. Upstream's audit explains
why setting alpha and then blocking in the same run-loop turn is insufficient
(`src/main-thread-ipc.md:67-75`).

Permissions are explicit adapter state: AX is required for global switching;
Screen Recording may be skipped while retaining icon/title operation. Prompt
only from onboarding/user action, cache status for the interaction path, and
recheck after returning from System Settings or receiving permission changes.
Use public trust/capture preflight APIs first; keep any private notification
optimization optional. On AX loss, unregister input and close the session;
on recording loss, stop capture, clear images, and offer permission recovery.
These are rewrite safety policies, not a claim that upstream clears all cached
pixels on revocation. Never treat a timeout as proof of permission denial.

## Capture and memory budget

All numbers here are **targets**, not measured RSS or a guarantee about Apple
framework allocations. MiB means 1,048,576 bytes; MB is decimal.

| Resource/state | Initial target or hard policy |
|---|---|
| Steady hidden RSS | <20 MiB target, after 30 seconds idle; report actual RSS and physical footprint separately |
| Thumbnail cache | 32 MiB default hard charged-byte limit; configurable 0–128 MiB |
| Hidden thumbnail bytes | 0 after hide/ownership cleanup; no screenshot disk cache |
| Typical visible working set | <80 MiB RSS target for 20 windows on a Retina display |
| Large captures in flight | 1; no unbounded concurrent ScreenCaptureKit requests |
| Reusable scaling scratch | At most 8 MiB owned by us; drop on hide |
| App-icon cache | 2 MiB decoded-pixel cap, deduplicated by application identity |
| Capture cadence | Demand-driven; at most 5 Hz selected tile, 1 Hz other visible tiles |
| Idle capture cadence | Zero; no streams or screenshot timers while hidden |

The 32 MiB cache is not a 32 MiB process limit. WindowServer/ScreenCaptureKit,
IOSurfaces, CoreAnimation copies, framework mappings, stacks, and allocator
retention may exceed the managed cache. Measure them separately. If native idle
baseline exceeds 20 MiB, document the measured baseline and contributors; do not
claim success by reporting only Rust heap allocations.

### Pixel representation and admission

**Decision:** retain display-sized premultiplied BGRA8: 8 bits **per channel**,
32 bits per pixel. Do not use 8-bit indexed color or grayscale: both destroy
preview fidelity. Do not PNG/JPEG-compress the live cache: decoding latency,
CPU work, and an additional decoded copy defeat cheap switching. With eviction
on hide there is no cold screenshot tier that justifies compression.

For a 240×150-point tile at 2×, 480×300×4 = 576,000 bytes before row alignment.
A 64-byte-aligned 1,920-byte row still costs 576,000 bytes; 32 MiB fits at most
58 such images with no other charged surfaces. A 640×400-pixel thumbnail costs
1,024,000 bytes and permits 32 entries. Charge actual `bytes_per_row * height`,
including owned backing storage; deduplicate shared backing identities.
Budget any separately retained platform image backing; never count handles as
if their pixel storage were free. CoreAnimation/compositor copies outside our
ownership are measured overhead, not silently charged as zero RSS.

Ask ScreenCaptureKit for the display pixel dimensions rather than capturing a
full-resolution image and keeping it. Fit/crop using the source aspect ratio and
backing scale. A transient full-size fallback image must be released immediately
after downscale; skip capture if safe transient bounds cannot be met. Avoid
IOSurface retention beyond the callback/transfer interval. AppKit receives an
owned display-sized CGImage only; no duplicate persistent NSImage wrapper.

The baseline one-shot binding is
`SCScreenshotManager::captureImageWithFilter_configuration_completionHandler`
with SCContentFilter and SCStreamConfiguration, not the newer
`captureScreenshot` selector used by this upstream revision on macOS 26.
The pinned [binding source](https://docs.rs/objc2-screen-capture-kit/0.3.2/src/objc2_screen_capture_kit/generated/SCScreenshotManager.rs.html)
exposes the CGImage completion and required feature gates. Check selector/SDK
availability on the deployment target before calling; a generated declaration
alone is not an availability check. Use SDR output and configured dimensions.

The pure core cache policy tracks key, pixel variant, byte cost, last-use epoch,
and readiness. It returns evicted keys; the adapter owns/drops native payloads.
Reserve the replacement cost before capture. On failure, release the reservation;
on completion, reconcile actual cost and evict before attaching. An item larger
than the entire budget is rejected, leaving the icon/title fallback.
Select victims by least recent displayed use, not metadata updates. Prefer
selected and visible tiles, but no pinning may violate the hard cap. If all
visible tiles exceed the cap, downscale or show placeholders rather than grow.

### Refresh and teardown

Capture only when visible and thumbnail style is active. Prioritize selected
then visible uncached/stale tiles; coalesce resize/title/visibility invalidation.
The cadence above is a maximum, not a global polling loop. A single deadline
scheduler covers demanded refreshes; unchanged frames may be retained for the
session. Icon/title modes never start capture. Minimized, protected, or
uncapturable windows keep an icon/title fallback; do not expose unrelated pixels.

On hide: increment generation; cancel pending demands; stop any capture stream
if one was used; clear layer contents and preview surfaces; evict all thumbnail
entries; release CGImage/CFData/IOSurface and scaling buffers; drain autorelease
pools. An in-flight request cannot necessarily be cancelled: its completion must
release results without repopulating cache. Retain only lightweight metadata,
selection history, and the bounded app-icon cache. Do not retain NSImage objects
for hidden window screenshots.

Use an autorelease pool for every off-main SDK job/callback batch and a bounded
pool scope in any custom run loop. Never hold autoreleased arrays across an
asynchronous boundary without explicit ownership. Preallocate/reuse tile arrays,
geometry vectors, and conversion buffers. Selection/repaint paths allocate no
new pixel buffers and no per-frame temporary collections after warm-up; image
acquisition may necessarily allocate inside Apple frameworks. Reuse rendering
objects rather than promising control over those opaque allocations.

## Config schema and persistence

**Decision:** the following is a schema example, not a shipped configuration.
Explicit defaults are centralized in core, not duplicated in the AppKit UI.

```toml
schema_version = 1
launch_at_login = false
menubar_icon = "outlined"
menubar_visible = true

[appearance]
style = "thumbnails"
size = "auto"
theme = "system"
screen = "active"
nominal_rows = 4
max_width_ratio = 0.85
max_height_ratio = 0.8
title_content = "window_title"
title_truncation = "end"

[capture]
cache_mib = 32
selected_max_hz = 5
other_max_hz = 1
evict_on_hide = true

[[shortcuts]]
id = "all_windows"
hold = ["option"]
next = "tab"
previous = "shift"
release_action = "focus"
apps = "all"
spaces = "all"
screens = "all"
minimized = "show"
hidden = "show"
fullscreen = "show"
windowless = "show_at_end"
order = "recently_focused"

[[shortcuts]]
id = "active_app"
hold = ["option"]
next = "key_above_tab"
previous = "shift"
release_action = "focus"
apps = "active"
spaces = "all"
screens = "all"
minimized = "show"
hidden = "show"
fullscreen = "show"
windowless = "show_at_end"
order = "recently_focused"

[[blacklist]]
bundle_id_prefix = "com.example.Excluded"
hide = "always"
suppress_shortcuts = "never"
```

Schema policy: `apps` permits all/active/non_active/windows_under_cursor/
app_under_cursor; `spaces` all/visible/non_visible; `screens` all/switcher_screen.
`order` permits recently_focused/recently_created/alphabetical/space. Blacklist
`hide` permits never/always/windowless/title_contains with a `title_patterns`
string array; `suppress_shortcuts` permits never/always/fullscreen. Appearance
also supports icons/titles, small/medium/large, light/dark, and mouse/menubar
placement. Release actions are focus/hold/search; search keeps the session open
and uses the editing/arrow/Escape precedence in SPEC. Title content supports
window_title/app_name/app_and_window; truncation supports start/middle/end.
A full settings UI exposes these enums rather than a second schema.

This example uses an explicit width override; absent that override, apply the
observed physical-display width heuristic. `nominal_rows` is an optional override
for thumbnail layout; absent it, use style/size/orientation presets. Login defaults
off by rewrite policy (upstream registers on); enabling it uses SMAppService
rather than writing a LaunchAgent. ScreenCaptureKit on macOS 14+, zero hidden
captures, and eviction-on-hide are other deliberate architectural departures.

Shortcuts use canonical names, not localized glyphs. Resolve keyboard-layout
keycodes in the adapter and preserve user intent on layout change. Additional
profiles use the same schema; UI errors identify duplicate IDs/conflicting
chords. Unknown enum values, unsupported schema versions, invalid ratios,
negative/overflowing budgets, and malformed TOML are errors, not silent resets.
Missing optional fields take documented defaults. Do not import upstream
UserDefaults implicitly or reuse its application/bundle identity.

Load from `~/Library/Application Support/alttab-rs/config.toml` on macOS.
A missing file yields defaults; an unreadable/invalid file is preserved and shown
in settings, with the last valid in-memory configuration retained. Core accepts
an injected path so parse, permissions, and write failures can be tested on Linux.

Save a validated snapshot to a uniquely created sibling temporary file, flush
and sync it, then atomically rename to the destination on the same filesystem.
Serialize saves through one owner and sync the parent directory where supported.
Never truncate the existing file before serialization succeeds. Do not write
capture data, AX handles, observed window titles, or usage telemetry to config.
Preferences UI changes flow through core validation before live application.
Login-at-launch registration is an explicit macOS effect, not implied success
from saving a boolean; surface registration failures and actual OS status.

## Upstream-to-Rust mapping

The upstream file paths below are source locations, not a requirement to copy
class boundaries. SPEC supplies behavior-level citations for these groups.

| Upstream component | Rust owner | Boundary |
|---|---|---|
| `src/App.swift`, `src/switcher/SwitcherSession.swift` | `core::session`, `macos::app` | Pure transitions versus executing UI/OS effects |
| `src/events/KeyboardEvents.swift`, `src/switcher/ATShortcut.swift`, `src/switcher/KeyRepeatTimer.swift` | `macos::input`, `core::session` | Raw interception versus repeat/cycle semantics |
| `src/switcher/state/Window.swift`, `src/switcher/state/Application.swift`, `src/switcher/state/Windows.swift`, `src/switcher/state/Applications.swift` | `core::model`, `macos::tracking` | Plain identity/state versus native ownership |
| `src/window-tracking/AttentionModel.swift`, `AttentionEngine.swift` | `core::order`, `macos::tracking` | Attention policy versus observed input/focus |
| `src/windowserver/WindowServerQuery.swift`, `WindowSurfaceInventory.swift`, `WindowElementAcquisition.swift` | `macos::tracking`, `macos::ffi::{skylight,ax}` | Surface discovery and AX acquisition |
| `src/events/WindowServerEvents.swift` | `macos::tracking`, `macos::spaces` | OS notifications and reconciliation |
| `src/macos/AXCallScheduler.swift`, `CGSCallScheduler.swift`, `ProcessCallScheduler.swift` | `macos::app` bounded lanes | Isolation, coalescing, timeout ownership |
| `src/preferences/Preferences.swift`, `MacroPreferences.swift` | `core::config`, `core::filter`, `macos::settings` | Schema/policy versus native controls |
| `src/switcher/Appearance.swift`, `src/switcher/main-window/` | `core::layout`, `macos::ui` | Geometry versus AppKit rendering |
| `src/macos/SystemPermissions.swift`, `src/secondary-windows/permission-window/` | `macos::permissions` | Cached status, user prompts, recovery |
| `src/macos/LoginItem.swift`, `src/Menubar.swift` | `macos::settings`, `macos::ui` | OS registration and status item |
| `src/switcher/state/WindowThumbnails.swift`, `src/events/WindowCaptureEvents.swift`, focus in `src/switcher/state/Window.swift` | `macos::capture`, `core::thumbnail`, `macos::focus` | Native images/focus versus cache policy |

No mapping for `src/pro`, Sparkle, non-English localization, or telemetry/crash
reporting: these are excluded, not stubbed subsystems awaiting implementation.

## Verification plan and unresolved risks

**Phase 1 verification:** source revision and license identity; executable
citation/path checks; TOML example parse; dependency metadata queries. No macOS
runtime or Rust application compilation can be claimed from these checks.

**Implementation acceptance on Linux:**

- Model creation/destruction and PID/window-ID reuse reject stale events.
- MRU seed, explicit focus, same-app activation, stable ties, and show-at-end
  partitions; failed AX reads preserve records.
- Cross-product filtering for minimized/hidden/fullscreen and Space/display/app
  scopes, blacklists, and windowless placeholders.
- Summon-delay-release, repeat deduplication, reverse traversal, row geometry,
  Esc, profile change, removal of selection, empty list, and late callbacks.
- Layout bounds and nonoverlap for tiny/huge screens, Retina scales, extreme
  aspect ratios, every style, and large window counts.
- TOML defaults/round trip/version rejection, permission/write failures, and
  preservation of the previous file on failed save.
- LRU replacement, reservations, oversized frames, byte accounting, budget
  shrink, generation invalidation, and zero cache bytes after hide.

Tests assert consumer-visible outcomes and invariants, not wiring or copied
constants. Feed recorded normalized event sequences into the actual core state
machine; a fake OS adapter cannot establish macOS integration correctness.

**Implementation acceptance on macOS:**

- Signed app on supported Intel and Apple Silicon versions; grant, deny, revoke,
  and regrant AX/Screen Recording permissions; test keyboard-layout changes.
- Exercise real Alt/Option and Cmd profiles, fast release, repeats, Esc, and
  switching between two apps and two windows of the same app.
- Test minimized/hidden/fullscreen windows, multiple Spaces/displays, display
  removal, active-Space changes, app launch/quit, AX failures, and private-symbol
  unavailability. Record capability degradation explicitly.
- Observe actual panel positioning, scrolling, title/icon fallbacks, accessibility,
  selected-window focus, and hidden-panel input delivery; tests alone are not UI
  proof. Include rapid hide/reopen while capture completes.
- Measure release-build RSS/physical footprint at startup and after 30 seconds
  idle, 20 visible windows, 100 windows, 100 show/hide cycles, and 60 seconds
  capture. Use Instruments Allocations/Leaks, `vmmap`, and native process metrics.
  Check IOSurface/CGImage counts and autorelease accumulation, not just heap size.
- Prove no captures while hidden, hard managed-cache limits, bounded in-flight
  work, no monotonically growing retained images, and smooth key-repeat latency.

**Unknowns:** private ABI compatibility across macOS releases, exact OS capture
memory overhead, protected/minimized capture availability, idle RSS feasibility,
SDK feature closure and final MSRV, and native focus reliability without private
APIs. These require implementation and macOS observation; static Swift source
and Linux dependency metadata cannot resolve them. Packaging/signing identity
must be chosen for alttab-rs before permissions testing and remain stable; do not
reuse upstream's identity or include its license/Keychain subsystem.
