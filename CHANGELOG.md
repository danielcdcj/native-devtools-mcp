# Changelog

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
