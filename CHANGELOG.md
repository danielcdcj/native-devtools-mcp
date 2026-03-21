# Changelog

## v0.6.0

### Chrome DevTools Protocol (CDP)

New `cdp_*` tools for automating Chrome and Electron apps via their remote debugging port, powered by the `chromiumoxide` crate. This eliminates the need for a separate Node.js MCP server (chrome-devtools-mcp) for browser automation.

- **`cdp_connect` / `cdp_disconnect`** — connect to a running Chrome/Electron instance on a given port
- **`cdp_take_snapshot`** — accessibility tree snapshot of the browser page (element UIDs, roles, names)
- **`cdp_evaluate_script`** — evaluate JavaScript in the page, with optional element references from the snapshot
- **`cdp_click`** — click a DOM element by UID (scroll-into-view, more reliable than screen coordinates for web content)
- **`cdp_hover`** — hover over a DOM element by UID
- **`cdp_fill`** — type text into an input/textarea or select an option from a `<select>` element
- **`cdp_press_key`** — press a key or key combination (e.g., `Enter`, `Control+A`, `Control+Shift+R`)
- **`cdp_type_text`** — character-by-character keyboard input into a focused element, with optional submit key
- **`cdp_handle_dialog`** — accept or dismiss JavaScript dialogs (alert, confirm, prompt)
- **`cdp_navigate`** — navigate to a URL, or go back/forward/reload (configurable timeout, handles slow-loading pages)
- **`cdp_new_page`** — create a new browser tab and navigate to a URL
- **`cdp_close_page`** — close a browser tab by index
- **`cdp_wait_for`** — wait for any of multiple texts to appear on the page (lightweight JS polling with timeout)
- **`cdp_list_pages` / `cdp_select_page`** — tab management

`cdp_click`, `cdp_hover`, `cdp_fill`, and `cdp_press_key` support `include_snapshot` to return a fresh snapshot with the action result, saving a round-trip.

Requires Chrome 136+: `--remote-debugging-port` is silently ignored unless `--user-data-dir` points to a non-default profile. The profile is persistent across launches.

Behind the `cdp` feature flag (enabled by default).

### Accessibility tree snapshot

New `take_ax_snapshot` tool that serializes the full macOS Accessibility (AX) tree into a structured text format with unique element IDs, roles, and names. Works for any app without requiring a debug port.

### Screen recording

New `start_recording` / `stop_recording` tools for capturing screen activity as MP4 video. Useful for recording UI flows, repro steps, and demo clips.

- Configurable FPS (default 5), region cropping, and max duration
- Supported on macOS (CGWindowListCreateImage) and Windows (BitBlt)

### Windows feature parity

Windows now supports all tools that were previously macOS-only:

- **Hover tracking** — `start_hover_tracking` / `get_hover_events` / `stop_hover_tracking` via UI Automation and GetCursorPos
- **Screen recording** — `start_recording` / `stop_recording` via BitBlt capture loop
- **`element_at_point`** — added `app_name` scoping and container fallback
- **`find_text`** — now searches UIA `Value` and `HelpText` properties in addition to `Name`
- **`get_cursor_position`** — new Windows implementation via GetCursorPos

### Fixes

- **Drag pre-move cursor** — cursor now moves to the start position before initiating a drag, ensuring correct start coordinates (Windows)
- **Hover dwell accuracy** — fixed dwell time calculation to use arrival/departure timestamps correctly, preventing inflated dwell values from pass-through elements
- **Frontmost app detection** — fixed macOS frontmost app resolution to use CGWindowList stacking order instead of NSWorkspace

### Other

- Windows code refactored: deduplicated PID resolution, extracted text property helper, simplified capture_window_jpeg and UIA element search
- Updated rustls-webpki to 0.103.10 (CVE fix)

## v0.5.1

### `element_at_point` improvements

- **AXSubrole** — `element_at_point` now includes the `subrole` field (from `AXSubrole`) in its response on macOS, giving LLMs finer-grained element classification (e.g., distinguishing "AXCloseButton" from a generic button)

### Hover tracking

- **Absolute timestamps** — hover events now use absolute Unix milliseconds instead of relative "ms since tracking started", making it easier to correlate events with external timelines and logs

## v0.5.0

### Hover tracking

New `start_hover_tracking` tool that continuously polls cursor position and the accessibility element under it, recording transitions as the user moves between UI elements. Designed for LLMs to observe user navigation patterns (e.g., tooltip triggers, dropdown reveals, panel expansions).

- **`start_hover_tracking`** — begins a polling session with configurable interval, max duration, and dwell threshold
- **`get_hover_events`** — drains buffered transition events (cursor position, element role/name/bounds, dwell time)
- **`stop_hover_tracking`** — ends the session and returns remaining events
- **Dwell threshold** (`min_dwell_ms`, default 300ms) — filters out pass-through elements during fast mouse movement, so only intentional hovers are recorded
- **Compact output** — element `value` field is dropped to avoid bloat (e.g., terminal buffers); remaining string fields are truncated to 100 chars. Use `element_at_point` with the event's cursor coordinates for full element details
- Tools appear dynamically: `get_hover_events` and `stop_hover_tracking` only show up while a session is active
- Supported on macOS and Windows

### App lifecycle tools

- **`launch_app`** — now accepts optional `args` parameter for CLI arguments (e.g., `--remote-debugging-port=9222`). Returns an error if the app is already running with args specified
- **`quit_app`** — new tool for graceful or force termination of running applications

### Fixes

- `list_apps` / `is_app_running` now uses `CGWindowListCopyWindowInfo` to supplement `NSWorkspace.runningApplications`, fixing stale data for recently launched apps
- `list_apps` filters to user-facing apps only, excluding system agents and daemons
- CI changelog extraction in release workflow fixed

## v0.4.4

### `element_at_point` tool

New tool that returns the accessibility element at given screen coordinates. Given an (x, y) point, returns the element's name, role, label, value, bounds, pid, and app_name. Optional `app_name` parameter scopes the lookup to a specific application (useful when windows overlap). Uses `AXUIElementCopyElementAtPosition` on macOS and `IUIAutomation::ElementFromPoint` on Windows.

### Fixes

- `element_at_point` now drills deeper into Electron/Chromium accessibility trees to return meaningful elements instead of top-level web area containers
- `verify` subcommand now detects source builds and shows an informational message instead of a checksum mismatch error

### Security

- Updated `aws-lc-sys` to 0.38.0 to resolve 3 high-severity vulnerabilities (PKCS7_verify signature validation bypass, PKCS7_verify certificate chain validation bypass, AES-CCM timing side-channel)

## v0.4.3

### `find_text` result ranking

Results are now ranked by relevance: exact matches appear before substring matches, and interactive elements (buttons, links, inputs) rank above static text. A `role` field (from AXRole on macOS, UIA ControlType on Windows) is included in the JSON output.

### `focus_window` fix for bundle-less apps

`focus_window` now reliably brings windows to front for apps without a proper macOS bundle (e.g., Tauri dev builds). After activation, it sets AXFrontmost and AXRaise via the Accessibility API as a fallback.

### Security & trust

- **`verify` subcommand** — hashes the running binary and checks it against official checksums from the GitHub release (exit 0 = verified, exit 1 = mismatch, exit 2 = inconclusive)
- **`setup` subcommand** — guided wizard that checks macOS permissions (Accessibility, Screen Recording) and auto-configures MCP clients (Claude Desktop, Claude Code, Cursor)
- **CI checksums** — every release now publishes `checksums.txt` with SHA-256 hashes for all binaries, archives, and the DMG
- **`SECURITY_AUDIT.md`** — documents which permissions are used, where in the code, and includes an LLM audit prompt
- **`scripts/build-from-source.sh`** — one-liner to clone, review, build, and set up from source

### CLI improvements

- Unknown commands and options now show an error and help text instead of silently starting the MCP server

### Docs

- Added Security & Trust section to README
- Restructured README: `setup` is now the primary post-install path, manual configuration collapsed into a details block

## v0.4.2

### Smarter `find_text` on empty results

When `find_text` (desktop) or `android_find_text` returns no matches, the response now includes an `available_elements` array listing all visible UI element names from the accessibility tree. This lets LLMs see what's actually on screen and retry with the correct name — solving the common issue where accessibility APIs use semantic names (e.g., "multiply" instead of "×", "All Clear" instead of "AC").

Applies to all platforms: macOS (Accessibility API), Windows (UI Automation), and Android (uiautomator).

### Fixes

- Fixed server.json description exceeding MCP Registry 100-character limit
- Removed outdated Android feature flag references from README

## v0.4.1

- Added MCP Registry publishing to the release workflow
- Server is now discoverable at `io.github.sh3ll3x3c/native-devtools` on the MCP Registry

## v0.4.0

### Android support included by default

Android device automation is now built into every release — no feature flags, no separate builds. Install from npm and the `android_*` tools are ready to use.

**Getting started:**
1. Connect an Android device with USB debugging enabled
2. Call `android_list_devices` to discover it
3. Call `android_connect(serial='...')` to unlock all Android tools

### Improved LLM instructions

Rewrote the server instructions that LLMs see to prevent tool misrouting between desktop and Android:

- **"Which tools to use" routing section** at the top — clear rules for when to use desktop vs Android vs app debug tools
- **Parallel workflow examples** for both platforms (find text → click)
- **Key differences called out** — no OCR on Android screenshots, no `focus_window` needed, absolute pixel coordinates
- Removed implementation jargon (CGEvent, AppDebugKit) in favor of plain descriptions

### Breaking changes

- The `android` Cargo feature flag has been removed. `adb_client` and `quick-xml` are now default dependencies. If you were building without `--features android`, your binary will now be slightly larger (~1MB) but functionally identical — Android tools remain hidden until you call `android_connect`.

## v0.3.6

- Accessibility API element tree search for `find_text` on macOS (faster, more accurate than OCR alone)
- Added `app_name` and `window_id` params to `find_text` for window-scoped search
- Added `launch_app` tool to start applications by name
- Disabled OCR language correction by default for better UI text detection
- `find_text` returns empty JSON array instead of prose on zero matches
- Windows compatibility fix for screenshot OCR
