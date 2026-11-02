# AltTab behavioral specification for alttab-rs

## 1. Scope, provenance, and evidence vocabulary

Reference: [lwouis/alt-tab-macos](https://github.com/lwouis/alt-tab-macos), revision
`c500294bdb01186011a5c64a412b04cef9931bef`, GPL-3.0.
The local source `/workspace/alttab-src` was read-only throughout this phase;
`git rev-parse HEAD` returned that revision. Citations below are relative to that
root: `src/App.swift:75-132` means inclusive source lines, not Rust files.
[Immutable source tree](https://github.com/lwouis/alt-tab-macos/tree/c500294bdb01186011a5c64a412b04cef9931bef).

**Observation (O):** executable source establishes a branch, data value, or call.
Unless explicitly labeled otherwise, sections 2–9 contain observations.
**Inference (I):** a consequence or reconstruction requirement derived from those
observations, not behavior exercised in a running app.
**Unknown (U):** a question source inspection does not settle or this bounded
investigation did not fully trace. Section 11 records these limitations.
**Decision (D):** a new Rust policy, recorded in ARCHITECTURE rather than presented
as upstream behavior. No app implementation is included in phase 1.

Method: the installed `reverse-engineer-anything` skill routes complete source
repositories to normal repository inspection. Searches and source reads traced
input → state → side effect. No REA binary provider was needed or started.
No macOS app was run on this Linux host; source timing constants are not measured
latencies, and source comments are not independent runtime proof.

In scope: keyboard switching, window/app tracking, ordering and filtering,
thumbnails, exact-window focus, native layout, essential preferences, permissions,
and onboarding. Related search and window actions are documented where they
change shortcut or selection semantics; this is not an exhaustive product manual.

Explicitly **out of scope**: everything under `src/pro` (including licensing and
upsell), Sparkle auto-updates, localization beyond English, telemetry, and crash
reporting. None is to be reimplemented or represented by placeholder behavior.
Some non-Pro preference accessors call excluded policy gates. The registered
values below are therefore **registered defaults**, not asserted entitlement-
independent effective defaults (`src/preferences/Preferences.swift:124-125,147`).
The rewrite implements the requested switching features without copying gates.

## 2. Global shortcuts and session lifecycle

### 2.1 Registered profiles and actions

Two keyboard profiles are registered by default; configurable count is 1–9.
A separate gesture slot uses index 9. Profile 0 holds Option and presses Tab,
showing all applications. Profile 1 holds Option and presses the physical key
above Tab (rendered through the current input source), showing the active app.
Extra profiles have Option hold but no trigger; profile 2's app-scope preset is
non-active apps. Cmd+Tab is configurable, not the registered default.
Sources: `src/preferences/Preferences.swift:8-9,50-65,152-158,380-385`.

| When-active key | Registered action |
|---|---|
| Return | Focus selected window |
| Shift alone | Previous window |
| Escape | Cancel, subject to search-state handling |
| W / M / F | Close / minimize-toggle / fullscreen-toggle |
| Q / H | Quit app / hide-toggle app |
| S | Toggle search |
| Arrow keys | Navigation, enabled by default |
| Vim h/j/k/l | Optional navigation, disabled by default |

Mouse-hover selection defaults off; display delay is 100 ms.
Sources: `src/preferences/Preferences.swift:10-25`;
`src/switcher/ShortcutAction.swift:8-35`;
`src/preferences/settings-window/tabs/controls/ControlsTab.swift:77-85,976-1021`.

Hold bindings are global/up-triggered; next-window bindings are global/down-
triggered; action bindings are local/down-triggered. A next binding combines
its own modifiers with the corresponding hold modifiers. Removing its trigger
unregisters it; a trigger can operate without a hold binding.
Source: `src/preferences/settings-window/tabs/controls/ControlsTab.swift:699-735,933-974`.

### 2.2 Carbon and event taps cooperate

Carbon registers actual keycode shortcuts with `RegisterEventHotKey`, dispatcher
target, signature `altt`, and retained hotkey references. Modifier-only shortcuts
are not Carbon registrations. Press/release handlers decode global IDs and route
to the common matcher. IDs 0–8 identify next bindings; 9–17 identify holds.
Sources: `src/events/KeyboardEvents.swift:5-11,172-204,257-278`; `src/events/KeyboardEventsTestable.swift:4-10`.

A permanent passive `.cgSessionEventTap` uses `.listenOnly`, observes
`.flagsChanged` and `.keyDown`, records physical event order, and forwards
modifier changes to main. Its key-down observations are not themselves matched
or swallowed. A separate active `.cghidEventTap` absorbs Escape only while a
session is active and any configured shortcut uses Escape. Other keys pass.
Sources: `src/events/KeyboardEvents.swift:30-83,156-164,229-249`.

Both taps use the dedicated keyboard/mouse/trackpad run loop. AppKit local
key-down/up monitors handle input delivered to the panel. Triggered non-hold
actions absorb input; dismissal-period input is discarded. Disabled taps are
re-enabled with state repair; failure to create a required tap restarts the app.
Sources: `src/events/KeyboardEvents.swift:141-164,207-216,229-254`; `src/events/KeyboardEventsTestable.swift:13-32,91-112`.

Cmd+Tab registration also suppresses native Cmd+Shift+Tab. Overlap checks collect
all matching symbolic hotkeys, including combined hold-modifier cases; they are
not first-match rules. The implementation uses `CGSSetSymbolicHotKeyEnabled`.
Normal termination restores native handling; no crash-recovery guarantee follows.
Sources: `src/preferences/settings-window/tabs/controls/NativeHotkeyResolver.swift:23-51`;
`src/preferences/settings-window/tabs/controls/ControlsTab.swift:743-750`;
`src/macos/api-wrappers/SkyLight.framework.swift:95-102`; `src/App.swift:732-734`.

### 2.3 Matching, cycling, and release

Hold matching requires at least the configured modifier bits: extra modifiers do
not release the hold. During a session, next-window matching also accepts the
trigger without the hold bits. Other actions match their own modifier set or
that set combined with the active hold. Fn is discarded from cleaned flags;
Command, Option, Control, Shift, and Caps Lock remain.
Sources: `src/switcher/state/ShortcutModifierResolver.swift:13-48`; `src/switcher/ATShortcut.swift:43-60,141-147`.

Release-to-focus requires an active session, matching profile, effective Focus
release style, and no forced hold-open flag. Local actions require a session.
Focus/Hold/Search release styles exist; Search enters editing at invocation and
forces hold-open, despite the internal name `searchOnRelease`.
Sources: `src/switcher/ATShortcut.swift:63-83`;
`src/preferences/MacroPreferences.swift:119-138`; `src/App.swift:420-425`.

Tab advances. Reverse traversal is not simply one hardcoded Shift+Tab chord:
the default Shift-only previous action, combined with modifier-aware next-key
matching, supplies the normal reverse interaction. Left/right traverse displayed
items; up/down choose geometrically in the neighboring row using horizontal
center. Hidden/non-displayed items are skipped. Manual navigation wraps;
repeat-driven navigation stops at boundaries instead of wrapping indefinitely.
Sources: `src/App.swift:319-332,450-453`;
`src/switcher/state/Windows.swift:431-450,474-487`;
`src/switcher/main-window/TilesView.swift:407-451,883-899`.

Synthetic repeat covers modifier-only previous bindings and eligible next
bindings. It uses system KeyRepeat/InitialKeyRepeat, falling back to 6/25 ticks
at 60 Hz: about 100 ms repeat and 417 ms initial delay. A one-shot timer rearms
after main consumes a tick. Late/pre-grace ticks are skipped and rearmed;
released shortcut/key/hold state stops it.
Sources: `src/switcher/KeyRepeatTimer.swift:17-67,77-118,122-138`; `src/switcher/KeyRepeatTimerTestable.swift:9-30,54-62`.

A bounded 64-record physical-order log handles tap/Carbon reordering and missing
release delivery. It ignores autorepeat presses, claims matching physical
presses, and limits release search to before the next switching press. The
matcher settles lost hold release before and after matching the current event,
using physical modifiers and recorded evidence rather than trusting callbacks
alone. Sources: `src/events/ModifierReleaseLog.swift:33-99,103-138,150-169`; `src/events/KeyboardEventsTestable.swift:78-112`; `src/switcher/ATShortcut.swift:99-114`.

### 2.4 Opening, selecting, accepting, and cancelling

Summon creates a session, captures existing IDs, selects the profile/screen,
filters and orders candidates, initializes selection, and schedules display.
Profile changes rebuild within the existing session. Delay callbacks validate
session identity. First summon with empty inventory requests discovery and waits
at least 400 ms; otherwise the configured delay applies.
Sources: `src/App.swift:32,388-480`; `src/switcher/state/Windows.swift:94-127`.

Initial MRU selection normally chooses the second displayed candidate, or the
sole candidate. If the attended window was filtered out, it chooses the first.
Before user navigation the default can settle as inventory arrives; afterwards
selection follows window identity through reorderings. Newcomers, replacement
windows, minimized top pairs, and non-MRU ordering have explicit resolver rules.
Sources: `src/switcher/state/SelectionResolver.swift:114-159,189-238,269-285`; `src/switcher/SwitcherSession.swift:37-61,73-77`; `src/switcher/state/Windows.swift:274-290`.

Return or qualifying hold release commits the target. Logical session state is
cleared before target focus. Mission Control modes can suppress target focus
while still dismissing. Escape cancels without a target-focus request; if search
was entered mid-session it first exits search, whereas a search-start profile
cancels outright. Sources: `src/App.swift:75-142,335-344`;
`src/switcher/ShortcutAction.swift:10-11,20-28,71-77`.

The panel can become transparent while temporarily remaining ordered in until
fronting is attempted, with a 100 ms fallback. Generation checks protect a new
summon from stale dismissal. Input gating/context-menu/trackpad resets start
immediately; search, Escape-tap, cursor, tooltip, and main-menu cleanup waits for
order-out and a later main turn. Sources: `src/switcher/main-window/TilesPanel.swift:86-159`;
`src/App.swift:82-125`.

Search routing gives IME/context-menu handling priority; otherwise arrows navigate
and Tab/Shift+Tab are swallowed. Printable action bindings without Cmd/Ctrl type
text when a nonempty hold binding exists; hold-modified actions remain available.
Sources: `src/switcher/state/SearchModeResolver.swift:83-124`; `src/switcher/main-window/TilesView.swift:148-164,252-282`.

## 3. Enumeration, identity, and ongoing tracking

### 3.1 Physical surfaces versus switchable destinations

Initial app inventory comes from `NSWorkspace.runningApplications`; continuous
changes use KVO. New arrivals wait 250 ms and pass PID-liveness checks. Startup
initializes Spaces/screens/observers and schedules a full refresh after one
second. A correction pass is throttled to one per second, not evidence of an
unconditional 1 Hz polling loop.
Sources: `src/switcher/state/Applications.swift:15-20,97-113`;
`src/events/RunningApplicationsEvents.swift:10-27,40-45`; `src/App.swift:539-558`.

Primary all-Space discovery enumerates CGS windows with and without invisible
options, then batch-queries `SLSWindowQueryWindows`. Rows include window/PID,
attributes, level, Space mask, title, parent, alpha, bounds, and tags. Stale scan
results are rejected. This is not public CGWindowList-only enumeration.
Sources: `src/switcher/state/Applications.swift:202-240`;
`src/macos/CGSCallScheduler.swift:93-100`; `src/windowserver/WindowServerQuery.swift:10-35`.

The physical surface inventory includes sheets and child surfaces that are not
independent switch destinations. It handles query issue ordering, quit-process
fences, and parent ancestry with cycle protection. Eligible surfaces subscribe
to WindowServer notifications before AX acquisition succeeds.
Sources: `src/windowserver/WindowSurfaceInventory.swift:103-175`;
`src/switcher/state/Applications.swift:249-278`.

AX supplies semantic meaning. Published AXWindows/focused/main projections are
matched to IDs; other-Space acquisition may scan private remote AX tokens.
Acquisition distinguishes found, positively absent, and no answer. Matching ID
alone is insufficient; a descendant element may expose its parent's window ID.
Sources: `src/windowserver/WindowElementAcquisition.swift:31-86`; `src/macos/api-wrappers/AXUIElement.swift:216-220,238-286`.

Admission rejects zero IDs and floating/system-dialog subroles; attached surfaces
represent parents. Discovery without semantics can remain latent. When placement
is evaluated, negative levels are rejected and higher levels require fullscreen
or AX-main evidence. Standard/main windows and described titled dialogs/roots may
be admitted; exact attention has separate rules.
Source: `src/switcher/state/WindowAdmissionResolver.swift:104-141,162-165,186-195,209-218`.

### 3.2 Observers and reconciliation

One AX observer belongs to each process generation, with a separate observer
generation. Enabled capabilities cover focused/main-window changes, title,
creation, and element destruction; focused-tab change is separately switchable
and off by default. Observer sources run on the AX-semantics run loop; callbacks
schedule blocking queries elsewhere, and teardown removes the source.
Sources: `src/window-tracking/AxObserverHealth.swift:27-87`;
`src/window-tracking/AxObserverRegistry.swift:161-191,295-350,413-465`.

Observer health supports retry/rebuild, recovery on app activation, and a sparse
30-second tick only while recovery is pending. AX calls use a one-second global
messaging timeout and separate first-try/scan/retry lanes with per-key coalescing.
Sources: `src/window-tracking/AxObserverRegistry.swift:209-237,353-408`; `src/macos/api-wrappers/AXUIElement.swift:11-16`;
`src/macos/AXCallScheduler.swift:61-94,128-137`; `src/events/WindowServerEvents.swift:58-68`.

SkyLight callbacks cover create/destroy/move/resize/focus/order-in/order-out and
Space membership/current-Space changes. Callback-scoped payloads are copied,
then drained on main with segmented geometry coalescing. Notification membership
is separate from admitted-window membership, enabling later rediscovery.
Sources: `src/windowserver/WsEventRouting.swift:8-46,78-98`;
`src/events/WindowServerEvents.swift:39-54,80-128,337-409`.

Ordered-out, minimized, app-hidden, phantom, and destroyed are distinct states.
Physical visibility is attribute bit 0x2; minimized uses tag bit 60; fullscreen
uses Space-mask bit 0x20. Hidden state comes from NSRunningApplication and
Workspace hide/unhide notifications, not merely surface tags.
Sources: `src/windowserver/WsWindowState.swift:32-70`;
`src/switcher/state/Application.swift:83-92`;
`src/events/WindowServerEvents.swift:71-75,330-335`.

AX-element destruction initiates delayed reconciliation, not immediate window
removal: it seeks replacement AX roots and independent surface evidence, with
150 ms initial delay and 2.15-second timeout. Confirmed closes create tombstones.
Public `CGWindowListCreateDescriptionFromArray` checks selected phantom IDs;
query failure is unknown and cannot justify deletion.
Sources: `src/switcher/state/Applications.swift:119-132,527-665`;
`src/macos/CGSCallScheduler.swift:52-56,85-90`.

Quit cleanup removes owned destinations, surface inventory, observers, attention,
scheduler state, and acquisition state. It matches NSRunningApplication identity
rather than relying only on a potentially changed PID.
Source: `src/switcher/state/Applications.swift:1222-1256`.

### 3.3 Spaces and screens

Topology includes current Space, indexes, visible Spaces across displays, and
screen-to-Space membership. It uses `CGSCopyManagedDisplaySpaces` and
`CGSManagedDisplayGetCurrentSpace`; optional per-Space enumeration builds an
inverted window membership map. Missing membership is backfilled using
`CGSCopySpacesForWindows`; nil is not flattened into an empty list.
Sources: `src/switcher/state/Spaces.swift:3-85`;
`src/switcher/state/Applications.swift:140-193`; `src/macos/CGSCallScheduler.swift:81-83`.

Space events extend a 0.5-second transition window; settled handling debounces
250 ms. Create discovery is suppressed during transition, but the executable
destroy branch still removes destroyed surfaces.
Sources: `src/events/WindowServerEvents.swift:132-148,318-323`; `src/switcher/state/WindowEventReducer.swift:44-75`.

With separate Spaces, display membership follows Space-map intersection;
otherwise it uses window rectangle intersection in Quartz coordinates.
Missing geometry/display information conservatively admits membership. Background
tabs may borrow sibling/focused-window geometry.
Source: `src/switcher/state/Window.swift:677-708`.

## 4. Ordering: MRU is not z-order

Attention is committed from semantic AX focus, directed annotated-session input,
and Workspace activation/focus answers. WindowServer focus/order/create/geometry
notifications and discovery do not themselves advance MRU. Directed input uses
a listen-only annotated-session type-13 tap with validated PID/window fields.
Sources: `src/window-tracking/AttentionDriver.swift:157-189`;
`src/events/WindowAttentionEvents.swift:54-103,127-183`.

AX attention settles per PID: 60 ms when recent hardware input is at most 0.5 s
old, otherwise 200 ms. Sequence/process-generation checks reject obsolete facts
and prevent background answers taking frontmost attention. Dense MRU ranks sort
focus timestamps, resolving ties by committed target then previous rank.
Sources: `src/window-tracking/AttentionSettlePolicy.swift:19-50`;
`src/window-tracking/AttentionModel.swift:26-88,111-118`;
`src/switcher/state/TrackedWindowState.swift:629-657`.

Z-order seeds startup history only, with 250 ms coalescing; first summon disables
further seeding. It does not fabricate focus timestamps. Alternative display
orders are recently created, alphabetical app/title, and Space. Alphabetical and
Space ties fall back to MRU. Search orders by match/relevance then MRU, bypassing
normal end buckets. Sources: `src/switcher/state/Windows.swift:500-535`; `src/App.swift:409-412`;
`src/switcher/state/TrackedWindowState.swift:678-702`;
`src/switcher/state/WindowOrderResolver.swift:54-108`.

## 5. Thumbnails, refresh, and caching

Capture is one-shot and trigger-driven, not continuous video streaming. Showing
requests thumbnails prioritized by viewport; external model refreshes and focused
window changes also request captures. Targeted background focused capture has
an 800 ms per-window throttle. Ordinary capture requires permission, unlocked
screen, capture-dependent style/preview, and background-enabled or active session
(or explicit force). Background capture is registered true.
Sources: `src/App.swift:354-359,469-479`; `src/preferences/Preferences.swift:44-45`;
`src/switcher/state/WindowThumbnails.swift:22-35,153-179`;
`src/switcher/state/TrackedWindowStateBridge.swift:189-193`.

| Upstream capture condition | Actual backend |
|---|---|
| macOS below 26 | Private `CGSHWCaptureWindowList` |
| macOS 26+, non-fullscreen | `SCScreenshotManager.captureScreenshot` |
| macOS 26+, fullscreen | `SCScreenshotManager.captureSampleBuffer` |

Sources: `src/switcher/state/WindowThumbnails.swift:174-179`;
`src/events/WindowCaptureEvents.swift:206-268,317-328`.
This is not a claim that ScreenCaptureKit first exists in macOS 26; it is this
revision's backend choice. `CGWindowListCreateImage` is not the selected backend
in these inspected capture branches. Public CGWindowList metadata serves other
purposes. Failed screenshot capture does not immediately fall back to sample-buffer
capture. Private capture requests best resolution/full size and ignores clip shape.

SCK caches SCWindow objects, coalesces missing-ID discovery, and requests shareable
content including offscreen windows. Discovery has a five-second timeout and
256 pending-request bound. SCK admission permits two counted captures and 256
waiters; its five-second watchdog releases accounting slots, not OS operations.
The synchronous private backend shares accounting but bypasses that admission
limit. Thumbnail/preview keys have a 200 ms throttle, not a promised frame rate.
Sources: `src/events/WindowCaptureEvents.swift:9-13,95-176,191-240,374-474`;
`src/switcher/state/Applications.swift:19-20`.

SCK thumbnail pixels derive from logical size × backing scale, downscaled to
tile size without upscaling; the older private path requests full-size images.
Visible descendant surfaces can enlarge capture bounds. Restore capture is
excluded for 0.7 s; SCK frames under 90% of expected size receive up to three
250 ms retries for thumbnails, then accept; partial full-resolution previews are refused.
Sources: `src/switcher/state/WindowThumbnails.swift:44-100,108-147`;
`src/events/WindowCaptureEvents.swift:58-92,279-288`;
`src/windowserver/WindowSurfaceInventory.swift:67-82`.

Each Window retains a thumbnail and capture area. Full-resolution previews are
session-local, LRU-capped at ten, requested for selection and two neighbors on
either side on macOS 26+. Preview falls back to an upscaled thumbnail. Ordinary
thumbnail storage is not a global byte-budgeted LRU and does not evict on every
hide; removal clears tile/visible-preview-panel references and capture bookkeeping.
Sources: `src/switcher/state/Window.swift:41-45,245-250`;
`src/switcher/SwitcherSession.swift:92-113`;
`src/switcher/state/WindowThumbnails.swift:6-18,186-195`;
`src/switcher/state/Windows.swift:453-471,843-905`.

A tile without a screenshot uses the application icon. Hidden/offscreen changes
can preserve a previous frame rather than acquiring fresh content. This does not
establish that every minimized/hidden window is capturable on every macOS build.
Sources: `src/switcher/main-window/TileView.swift:323-338`;
`src/switcher/state/WindowEventReducer.swift:1302-1396`.

## 6. Focusing the selected target

Own windows activate AltTab and use `makeKeyAndOrderFront` with a closed-window
guard. Windowless apps use NSWorkspace openApplication, falling back to
NSRunningApplication activation; targets without a window ID activate all app
windows. External real windows follow a different exact-window path.
Source: `src/switcher/state/Window.swift:362-414`.

That path captures a generation-checked focus intent and runs on the AX command
queue: unminimize if needed; convert PID to PSN; call
`_SLPSSetFrontProcessWithOptions` for the target ID; notify main of attempted
fronting; send a private make-key event; AX raise with reacquisition/rebinding
on failure; repair cross-Space origin-front state; check supersession and outcome.
Sources: `src/switcher/state/Window.swift:436-459,544-601`.

The private key-window packet is a single synthetic mouse-down event posted by
`SLPSPostEventRecordTo`, with an out-of-window point and target ID. It is **not**
a mouse-down/up pair despite a high-level comment. AX absence does not prevent
the first private front/key steps. Cross-Space origin repair is used only when
membership is known and invokes `SLSSpaceSetFrontPSN`.
Sources: `src/macos/api-wrappers/SkyLight.framework.swift:141-175,184-194`;
`src/switcher/state/Window.swift:394-410,544-583`.

After 150 ms, public CGWindowList z-order and front PID help detect a covered
target. A conditional second AX raise revalidates intent and focused window with
a 250 ms AX timeout; it does not repeat synthetic key delivery. Actual observed
focus feeds attention/MRU; an attempted fronting callback is not proof of success.
Sources: `src/switcher/state/Window.swift:474-535`;
`src/macos/api-wrappers/CGWindow.swift:5-7`;
`src/events/WindowServerEvents.swift:280-312`.

## 7. Switcher UI and multi-screen layout

The main surface is a key-capable nonactivating NSPanel at pop-up-menu level.
Styles are Thumbnails, App Icons, and Titles; sizes Small/Medium/Large/Auto.
Thumbnail tiles show imagery plus icon/title/status; App Icons hides ordinary
labels/status and reveals the focused/hovered label; Titles uses small icons and
text. Icon style does not inherently group all windows into one app item.
Sources: `src/switcher/main-window/TilesPanel.swift:4,26-36`;
`src/preferences/MacroPreferences.swift:307-335`;
`src/switcher/Appearance.swift:68-75,93-185`;
`src/switcher/main-window/TileView.swift:215-250,310-335,755-774`;
`src/preferences/Preferences.swift:257-259`; `src/switcher/state/Windows.swift:129-144`.

Height limit is 80% of full screen frame. Width ratio is
`clamp(600 / physical_width_mm, 0.45, 0.9)`, or 0.9 when physical size is unknown.
Physical/logical aspect disagreement can invalidate physical size. Content
subtracts style padding; final placement uses visibleFrame, not full frame.
Sources: `src/switcher/Appearance.swift:20-23,51-57`;
`src/switcher/AppearanceTestable.swift:12-17`;
`src/switcher/state/Screens.swift:99-118`.

Thumbnail nominal rows are 5/4/3 for Small/Medium/Large on landscape and 8/7/6
on portrait. They determine tile height, **not a hard maximum row count**.
Nominal icons are 16/26/28 points; App Icons uses 70/110/150; Titles 18/24/30.
Thumbnail tile height is rounded from `(maxContentHeight - 1) / rows - 1`.
Images preserve aspect ratio and ordinary small windows are not enlarged.
Sources: `src/switcher/Appearance.swift:93-183`;
`src/switcher/main-window/TileView.swift:714-752,768-774`.

Grid layout uses fixed tile height and variable widths, wraps at available width,
records row indexes, and centers rows. Auto tries Large → Medium → Small, keeping
Small plus scrolling if none fits. Viewport height is capped; document height is
not. Titles have an additional readability-width cap, not longest-title sizing.
Sources: `src/switcher/main-window/TileGridLayout.swift:39-108`;
`src/switcher/main-window/TilesView.swift:453-509,515-655`;
`src/switcher/main-window/TileView.swift:701-705`; `src/switcher/main-window/TilesPanel.swift:178-194`.

Placement choices: Active = attended window's cached screen, falling back to
active menubar display; Mouse = containing screen; Menubar = first NSScreen.
Failure falls back to first screen; a transient empty screen list preserves the
previous preference. Panel centers in visibleFrame. Search shrinkage preserves
top-center until it exceeds its previous high-water height. Display changes
trigger screen/Space/layout refresh with 200 ms throttling.
Sources: `src/switcher/state/Screens.swift:16-71,112-118`;
`src/switcher/main-window/TilesPanel.swift:65-84`;
`src/events/ScreensEvents.swift:4-24`.

## 8. Preferences and filtering essentials

Preferences register UserDefaults values and cache typed reads; notified mutations
invalidate caches and dispatch side effects. Style/size/theme/release/preview
may have per-profile overrides, whose presence depends on persisted keys.
Sources: `src/preferences/Preferences.swift:160-175,197-250,263-284,318-344,490-527`;
`src/events/PreferencesEvents.swift:64-87`.

Registered appearance defaults: Thumbnails, Auto size, System theme, Active
screen, Window Title, end truncation, preview off. Title choices independently
select app name, window title, or their combination (equal names appear once);
truncation supports start/middle/end.
Sources: `src/preferences/Preferences.swift:26-44`;
`src/switcher/main-window/TileView.swift:627-640`;
`src/switcher/main-window/TileTitleView.swift:78-85`.

Per-profile defaults show all Spaces/screens and minimized/hidden/fullscreen
windows; windowless apps show at end; all windows are separate app entries and
native tabs initially share one representative. Minimized/hidden/windowless
policies allow Show/Hide/Show at end; fullscreen UI offers Show/Hide only.
Outside search, end-partition comparisons check windowless, then hidden, then
minimized; each marked category sorts later, not in that front-to-back sequence.
Sources: `src/preferences/Preferences.swift:54-65`;
`src/preferences/settings-window/tabs/controls/ShortcutEditor.swift:324-340`;
`src/switcher/state/WindowOrderResolver.swift:54-76`.

Filters compose phantom/exception rejection, app scope, hidden-app policy, then
real-window fullscreen/minimized/Space/screen/tab/cursor gates. Windowless
placeholders bypass real-window gates but not app scope/hidden/exceptions, and
cannot satisfy Windows under cursor.
Space choices are All/Visible/Non-visible; visible tests all displays' visible
Spaces. Temporarily held tabs with no Space IDs have explicit membership relief.
Sources: `src/switcher/state/WindowFilterResolver.swift:15-58,75-88`;
`src/preferences/MacroPreferences.swift:189-200`.

App scope includes All/Active/Non-active/Windows under cursor/App under cursor.
Cursor-window scope excludes minimized, hidden, and off-Space windows; app under
cursor uses front-to-back normal nontransparent surfaces. Screen filtering and
switcher placement remain independent controls.
Sources: `src/preferences/MacroPreferences.swift:171-186`;
`src/switcher/state/WindowFilterResolver.swift:61-74`;
`src/switcher/state/Screens.swift:24-64`.

Blacklist UI is called Exceptions. Bundle IDs match nonempty case-sensitive
prefixes. Hide modes: None/Always/When no open window/Window title contains.
Title patterns are literal case-sensitive OR substrings, not regex/globs.
Ignore-shortcuts modes independently select None/Always/When fullscreen.
Active-app/app-under-cursor requests bypass blanket hiding but not title rules.
Sources: `src/preferences/MacroPreferences.swift:410-441`;
`src/switcher/state/ExceptionMatcher.swift:38-64`;
`src/switcher/state/WindowFilterResolver.swift:35-45`.

Ignore-shortcuts exceptions unregister globals and dismiss an open session.
Defaults hide windowless Finder and a McAfee Safari host; remote-desktop/VM
applications have fullscreen suppression entries. New picker entries default to
Always hide without shortcut suppression.
Sources: `src/App.swift:490-510`; `src/preferences/Preferences.swift:387-402`;
`src/events/KeyboardEvents.swift:127-139,172-185`;
`src/preferences/settings-window/tabs/ExceptionsTab.swift:219-231`.

Launch at login defaults on. Upstream writes/removes a user LaunchAgents plist
with executable path, RunAtLoad, Aqua session, associated bundle ID, and
Interactive process type; it does not use SMAppService registration here.
Reconciliation runs after initialization and on preference changes. Errors log
without rollback; removing a legacy login item can re-enable a disabled preference.
Sources: `src/preferences/Preferences.swift:34`; `src/macos/LoginItem.swift:10-81`;
`src/events/PreferencesEvents.swift:56-70,80-85`.

Menu icon defaults visible/outlined; six current/legacy variants include colored
non-template icons. Left/accessibility click opens menu; right click summons
profile 0. Command-drag removal persists hidden state; icon is 22×22 points.
Menu offers permissions recovery; missing capture permission can show a callout
when any slot 0–9 (including inactive slots) depends on screenshots or preview.
Sources: `src/preferences/Preferences.swift:35-36,367-376`;
`src/preferences/MacroPreferences.swift:1-11`; `src/Menubar.swift:41,129-165,194-220`;
`src/macos/PermissionCalloutResolver.swift:33-52`.

## 9. Permissions and onboarding

Accessibility is mandatory. Screen Recording can be granted or explicitly
skipped with Continue without thumbnails; skipped is not an OS grant and does
not rewrite appearance preferences. AX is the first live step, then recording.
Startup continues once, only after AX granted and recording granted/skipped.
Sources: `src/secondary-windows/permission-window/PermissionFlow.swift:6-9,40-71`;
`src/macos/SystemPermissions.swift:79-96,239-254`; `src/App.swift:514-527`.

Before startup completes, onboarding cannot simply be dismissed: close/Cmd-W
terminates, Quit remains available, and standard window controls are hidden.
After startup it can close normally. Choosing Grant after skipping clears the
skip preference before OS success; detecting a grant alone need not clear it.
Sources: `src/secondary-windows/permission-window/PermissionsWindow.swift:23-29,51-72,146-149,223-238`;
`src/macos/SystemPermissions.swift:200-203,216-224,251-254`.

Checks are notification-driven via `com.apple.tcc.access.changed`, initial check,
coalescing, and bounded retries—not the polling described by an old launch comment.
AX detection prefers private TCC audit-token checking, falling back to
AXIsProcessTrustedWithOptions without prompt. Request versus opening System
Settings depends on whether a TCC entry already exists.
Source: `src/macos/SystemPermissions.swift:18-76,108-139,160-176`.

Recording detection first uses another process's visible normal-window title;
the skipped branch uses CGPreflightScreenCaptureAccess. No TCC entry returns
not granted; otherwise checks use SCShareableContent on 12.3+ or display streams.
A six-second wait timeout yields unknown and preserves prior state without
cancelling the OS operation; retries use 1/2/4 seconds with generation checks.
Sources: `src/macos/SystemPermissions.swift:68-76,200-231,258-341`;
`src/macos/PermissionCheckRetry.swift:9-20`.

AX revocation after startup queues restart. Recording revocation does not
restart/reblock startup; permission-dependent capture/preview stop admitting work
and the menu callout updates. Existing cached-pixel erasure is not proven by
those gates alone. Sources: `src/macos/SystemPermissions.swift:50-64,98-103`;
`src/switcher/state/WindowThumbnails.swift:6-19,153-159`.

## 10. Inferences and reconstruction obligations

- **I1:** distinguish physical surfaces, switch destinations, and AX roots; one
  flat window list cannot preserve parent, discovery, and replacement semantics.
- **I2:** preserve failed/unknown versus absent queries, generation fences, and
  selected identity. Timeout-driven deletion would contradict observed recovery.
- **I3:** public app activation plus AX raise alone is not demonstrated equivalent
  to upstream's private exact-window and cross-Space focus sequence.
- **I4:** capture freshness has no fixed FPS contract. A 32 MiB LRU, visible-only
  capture, and eviction on hide are deliberate Rust changes, not upstream facts.
- **I5:** keep appearance, app grouping, filtering, and permission status separate;
  changing style must not silently alter which windows exist or count as eligible.
- **I6:** AppKit mutations stay on main; delaying bookkeeping after visible work
  matters because a CoreAnimation transaction commits at run-loop-turn end.
  Upstream records this constraint in `src/main-thread-ipc.md:17-32,67-87`.

## 11. Unknowns, limitations, and phase-2 acceptance

**U1:** no native execution here: Secure Input, tap delivery races, OS-version
permissions, focus reliability, screen placement, and capture fidelity remain
macOS validation obligations. Source constants are not measured guarantees.
**U2:** excluded policy gates prevent claiming universal effective defaults or
feature entitlement. Registered values and non-excluded consumers are established.
**U3:** private ABI stability, notification IDs, bit masks, remote AX tokens, and
SLPS packet correctness require per-supported-OS validation before Rust calls.
**U4:** minimized/hidden/protected capture success and asynchronous frame freshness
are unproven. Late preview callbacks' affinity to a new session needs testing.
**U5:** this is not a full native-tab, observer-health retry, or close-verdict truth
table audit; those subsystems were traced at the requested feature boundaries.
**U6:** nominal rows are not max rows; oversized-first-tile grid arithmetic has an
empty-row edge case (`src/switcher/main-window/TileGridLayout.swift:48-68`). The
rewrite should bound geometry explicitly, not claim pixel-identical reproduction.
**U7:** launch-agent writing proves neither successful login launch nor System
Settings presentation. Private permission notifications may vary by OS release.

Phase-2 acceptance must exercise actual Option/Cmd invocation, forward/reverse/
row navigation, rapid release before display, Esc, profile changes, and focus;
app creation/quit, hung AX, ID reuse, minimized/hidden/fullscreen windows, multiple
Spaces/displays; all appearance/filter/exception settings; permission denial,
waiver, revocation/regrant; and capture completion racing hide/reopen. Linux can
prove deterministic core policy, not native integration. ARCHITECTURE defines
the Linux/macos test boundary and memory measurement plan.
