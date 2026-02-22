# Changelog

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
