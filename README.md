# native-devtools-mcp

A Model Context Protocol (MCP) server for testing native desktop applications, similar to how Chrome DevTools enables web UI testing.

> **100% Local & Private** - All processing happens on your machine. No data is sent to external servers. Screenshots, UI interactions, and app data never leave your device.

![Demo](demo.gif)

## Platform Support

| Platform | Status | Requirements |
|----------|--------|--------------|
| **macOS** | Supported | macOS 10.15+ (Catalina) |
| **Windows** | Supported | Windows 10 1903+ |
| **Linux** | Planned | - |

> Linux support will be added in a future release with platform-specific backends.

## Overview

This MCP server enables LLM-driven testing of native desktop apps by providing:
- **Screenshots** - Capture full screen, windows, or regions
- **Input simulation** - Click, type, scroll, drag via platform-native events
- **Window/app enumeration** - List and focus windows and applications

## Two Approaches to UI Interaction

This server provides **two distinct approaches** for interacting with application UIs. They are kept separate intentionally to give the LLM full control over which approach to use based on context.

### 1. AppDebugKit (`app_*` tools) - Element-Level Precision

For applications that embed [AppDebugKit](./AppDebugKit/), you get:
- **Element targeting by ID** - Click buttons, fill text fields by element reference
- **CSS-like selectors** - Query elements with `#id`, `.ClassName`, `[title=Save]`
- **View hierarchy inspection** - Traverse the UI tree programmatically
- **Framework-aware** - Works with AppKit and SwiftUI controls

**Best for:** Apps you control, AppKit/SwiftUI apps, when you need reliable element targeting.

```
app_connect → app_get_tree → app_query(".NSButton") → app_click(element_id)
```

### 2. Native Input (`click`, `type_text`, etc.) - Universal Compatibility

For **any application** regardless of framework:
- **Screen coordinate targeting** - Click at (x, y) positions
- **Works with any UI framework** - egui, Electron, Qt, games, anything
- **No app modification required** - Just needs Accessibility permission (macOS) or standard permissions (Windows)

**Best for:** Third-party apps, egui/Electron/Qt apps, when AppDebugKit isn't available.

```
take_screenshot → (analyze visually) → click(x=500, y=300)
```

### Why Two Approaches?

The LLM needs to choose the right approach based on what it observes:

| Scenario | Recommended Approach |
|----------|---------------------|
| App with AppDebugKit embedded | `app_*` tools - reliable element IDs |
| egui/Electron/Qt app | Native input tools - coordinate-based |
| Unknown app | Try `app_connect`, fall back to native input |
| App with poor view hierarchy | Native input even if AppDebugKit connected |

Merging them into auto-fallback would hide important context from the LLM and reduce its ability to make informed decisions.

## Installation

### Option 1: npm (Recommended)

```bash
# Install globally
npm install -g native-devtools-mcp

# Or run directly with npx
npx native-devtools-mcp
```

### Option 2: Build from source

```bash
# Clone the repository
git clone https://github.com/sh3ll3x3c/native-devtools-mcp
cd native-devtools-mcp

# Build
cargo build --release

# Binary location (macOS/Linux)
./target/release/native-devtools-mcp

# Binary location (Windows)
.\target\release\native-devtools-mcp.exe
```

> **Note (Windows):** Building from source requires Rust 1.88+ due to the `rmcp` crate requiring edition 2024. Use `rustup install 1.88.0` if needed.

## Required Permissions (macOS)

This MCP server requires macOS privacy permissions to capture screenshots and simulate input. **These permissions are required for the tools to function.**

### Step-by-Step Setup

#### 1. Screen Recording Permission (required for screenshots)

1. Open **System Settings** → **Privacy & Security** → **Screen Recording**
2. Click the **+** button (you may need to unlock with your password)
3. Add the app that runs Claude Code:
   - **VS Code**: `/Applications/Visual Studio Code.app`
   - **Terminal**: `/Applications/Utilities/Terminal.app`
   - **iTerm**: `/Applications/iTerm.app`
4. **Quit and restart the app completely** (not just reload)

#### 2. Accessibility Permission (required for click, type, scroll)

1. Open **System Settings** → **Privacy & Security** → **Accessibility**
2. Click the **+** button
3. Add the same app as above (VS Code, Terminal, etc.)
4. **Quit and restart the app completely**

#### 3. macOS Version (for OCR features)

The `find_text` tool and `take_screenshot` OCR feature use Apple's Vision framework for text recognition. This is the **recommended way** to interact with apps when AppDebugKit is not available.

- **macOS 10.15+ (Catalina)** required for OCR (`VNRecognizeTextRequest`)
- No additional software installation needed - Vision is built into macOS

### Important Notes

- **Grant permissions to the host app** (VS Code, Terminal), not to the MCP server binary itself
- **Restart is required** - Permissions don't take effect until you fully quit and reopen the app
- **No popup appears** - macOS won't prompt you; it silently fails if permissions are missing
- If you see `could not create image from display`, you need Screen Recording permission
- If clicks don't work, you need Accessibility permission

### During Automation (Important)

These tools assume the target window stays focused. If you use the mouse/keyboard, a macOS permission prompt appears, or Claude Code asks to approve a tool call, focus can change and actions may be sent to the wrong app or field.

- Pre-grant Screen Recording and Accessibility permissions before running.
- Pre-approve Claude Code tool permissions for this MCP server so no prompts appear mid-run.
- Avoid interacting with the computer while scenarios are executing.

### Privacy & Security

All data stays on your machine:
- Screenshots are captured locally and sent directly to Claude via the MCP protocol
- No data is uploaded to external servers
- The MCP server runs entirely offline
- Source code is open for audit

## Requirements (Windows)

Windows support requires Windows 10 version 1903 (May 2019 Update) or later.

### Permissions

Unlike macOS, Windows does not require explicit permission grants for most functionality:
- **Screenshots** - Work out of the box using GDI BitBlt
- **Input simulation** - Works via SendInput API
- **OCR** - Uses built-in Windows.Media.Ocr (WinRT)

### Limitations

- **Elevated processes** - Cannot capture screenshots of or send input to applications running as Administrator unless the MCP server is also elevated
- **DirectComposition/DXGI apps** - Some modern apps using hardware-accelerated rendering may appear as black rectangles in screenshots
- **UWP apps** - May have restrictions on input simulation

### OCR Requirements

The `find_text` tool uses Windows.Media.Ocr which is built into Windows 10 1903+:
- No additional installation required
- Automatically uses the system's language packs
- For best results, ensure your target language is installed in Windows Settings → Time & Language → Language

## MCP Configuration

### Getting started

Add to your Claude Code MCP config (`~/.claude/claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "native-devtools": {
      "command": "npx",
      "args": ["-y", "native-devtools-mcp"]
    }
  }
}
```

Note: Use `native-devtools-mcp@latest` if you want to always run the newest version.

### Build from source

```json
{
  "mcpServers": {
    "native-devtools": {
      "command": "/path/to/native-devtools-mcp"
    }
  }
}
```

## Tools

### System Tools (work with any app)

| Tool | Description |
|------|-------------|
| `take_screenshot` | Capture screen, window, or region (base64 PNG). Returns screenshot metadata for coordinate conversion and includes OCR text annotations by default (`include_ocr: true`). |
| `list_windows` | List visible windows with IDs, titles, bounds |
| `list_apps` | List running applications |
| `focus_window` | Bring window/app to front |
| `get_displays` | Get display info (bounds, scale factors) for coordinate conversion |
| `find_text` | Find text on screen using OCR; returns screen coordinates for clicking |

### Native Input Tools (work with any app)

| Tool | Description |
|------|-------------|
| `click` | Click at screen/window/screenshot coordinates (supports captured screenshot metadata) |
| `type_text` | Type text at cursor position |
| `press_key` | Press key combo (e.g., "return", modifiers: ["command"]) |
| `scroll` | Scroll at position |
| `drag` | Drag from point to point |
| `move_mouse` | Move cursor to position |

### AppDebugKit Tools (require app to embed AppDebugKit)

| Tool | Description |
|------|-------------|
| `app_connect` | Connect to app's debug server (ws://127.0.0.1:9222). Supports `expected_bundle_id` and `expected_app_name` validation. |
| `app_disconnect` | Disconnect from app |
| `app_get_info` | Get app metadata (name, bundle ID, version) |
| `app_get_tree` | Get view hierarchy |
| `app_query` | Find elements by CSS-like selector |
| `app_get_element` | Get element details by ID |
| `app_click` | Click element by ID |
| `app_type` | Type text into element |
| `app_press_key` | Press key in app context |
| `app_focus` | Focus element (make first responder) |
| `app_screenshot` | Screenshot element or window |
| `app_list_windows` | List app's windows |
| `app_focus_window` | Focus specific window |

Note: app_* tools (except `app_connect`) are only listed after a successful connection. The server emits a tools list change on connect/disconnect, so some clients may need to refresh/re-list tools to see the app_* set.

<details>
<summary><strong>Agent Context (for automated agents)</strong></summary>

This section provides a compact, machine-readable summary for LLM agents. For a dedicated agent-first index, see `agents.md`.

### Capabilities Matrix

| Intent | Tools | Outputs |
|--------|-------|---------|
| Capture screen or window | `take_screenshot` | base64 PNG, metadata (origin, scale), optional OCR text |
| Find text and click it | `find_text` → `click` | coordinates, click action |
| List and focus windows | `list_windows` → `focus_window` | window list, focus action |
| Element-level UI control | `app_connect` → `app_query` → `app_click` | element IDs, click action |

### Structured Intent (YAML)

```yaml
intents:
  - name: capture_screenshot
    tools: [take_screenshot]
    inputs:
      scope: { type: string, enum: [screen, window, region] }
      window_id: { type: number, optional: true }
      region: { type: object, optional: true }
      include_ocr: { type: boolean, default: true }
    outputs:
      image_base64: { type: string }
      metadata: { type: object, optional: true }
      ocr: { type: array, optional: true }
  - name: find_text_and_click
    tools: [find_text, click]
    inputs:
      query: { type: string }
      window_id: { type: number, optional: true }
    outputs:
      matches: { type: array }
      clicked: { type: boolean }
  - name: list_and_focus_window
    tools: [list_windows, focus_window]
    inputs:
      app_name: { type: string, optional: true }
    outputs:
      windows: { type: array }
      focused: { type: boolean }
  - name: element_level_interaction
    tools: [app_connect, app_query, app_click, app_type]
    inputs:
      selector: { type: string }
      element_id: { type: string, optional: true }
      text: { type: string, optional: true }
    outputs:
      element: { type: object }
      ok: { type: boolean }
```

### Prompt -> Tool -> Output Examples

| User prompt | Tool sequence | Expected output |
|-------------|---------------|-----------------|
| "Take a screenshot of the Settings window" | `list_windows` → `take_screenshot(window_id)` | base64 PNG, metadata, OCR text |
| "Click the OK button" | `take_screenshot` → (vision) → `click(screenshot_x/y + metadata)` | click action |
| "Find text 'Submit' and click it" | `find_text(query)` → `click(x,y)` | coordinates, click action |
| "Click the Save button in the AppDebugKit app" | `app_connect` → `app_query("[title=Save]")` → `app_click(element_id)` | element ID, click action |

### Coordinate Usage

| Coordinate source | Click parameters |
|-------------------|------------------|
| `find_text` or OCR annotation | `x`, `y` (direct screen coords) |
| Visual inspection of screenshot | `screenshot_x/y` + metadata from `take_screenshot` |

</details>

## How Screenshots and Clicking Work

### macOS

- **Screenshots** are captured via the system `screencapture` utility (`-x` silent, `-C` include cursor, `-R` region, `-l` window with `-o` to exclude shadow), written to a temp PNG, and returned as base64. Metadata for the screenshot origin and backing scale factor is included for deterministic coordinate conversion. Window screenshots exclude shadows so that pixel coordinates align exactly with `CGWindowBounds`, and OCR coordinates are automatically offset into screen space.
- **Clicks/inputs** use CoreGraphics CGEvent injection (HID event tap). This requires Accessibility permission and works across AppKit, SwiftUI, Electron, egui, etc.

### Windows

- **Screenshots** are captured via GDI BitBlt from the desktop DC. For window captures, DWM extended frame bounds are used to exclude invisible borders. The raw bitmap is encoded to PNG in-memory and returned as base64 with metadata for coordinate conversion.
- **Clicks/inputs** use the SendInput API with `INPUT_MOUSE` and `INPUT_KEYBOARD` structures. Text input uses `KEYEVENTF_UNICODE` for direct Unicode character injection, bypassing keyboard layout issues. This works across Win32, WPF, UWP, Electron, and most UI frameworks.

### Coordinate Conversion

On both platforms, window-relative or screenshot-pixel coordinates are converted to screen coordinates using captured metadata when available, otherwise window bounds and display scale are looked up at click time.

## Coordinate Systems and Display Scaling

The `click` tool supports multiple coordinate input methods. Choose based on how you obtained the coordinates:

### OCR Coordinates (from `find_text` or `take_screenshot` OCR)

OCR results return **screen-absolute coordinates** that are ready to use directly:

```json
// OCR returns: "Submit" at (450, 320)
// Use direct screen coordinates:
{ "x": 450, "y": 320 }
```

### Screenshot Pixel Coordinates (from visual inspection)

When you visually identify a click target in a screenshot image, use the pixel coordinates with the screenshot metadata:

```json
// take_screenshot returns metadata:
// { "screenshot_origin_x": 50, "screenshot_origin_y": 80, "screenshot_scale": 2.0 }
//
// You identify a button at pixel (200, 100) in the image.
// Pass both the pixel coords and the metadata:
{ "screenshot_x": 200, "screenshot_y": 100, "screenshot_origin_x": 50, "screenshot_origin_y": 80, "screenshot_scale": 2.0 }
```

### Other Coordinate Methods

```json
// Window-relative (converted using window bounds)
{ "window_x": 100, "window_y": 50, "window_id": 1234 }

// Screenshot pixels (legacy: window lookup at click time - less reliable)
{ "screenshot_x": 200, "screenshot_y": 100, "screenshot_window_id": 1234 }
```

### Quick Reference

| Coordinate source | Click parameters |
|-------------------|------------------|
| `find_text` result | `x`, `y` (direct) |
| `take_screenshot` OCR annotation | `x`, `y` (direct) |
| Visual inspection of screenshot | `screenshot_x`, `screenshot_y` + metadata |
| Known window-relative position | `window_x`, `window_y`, `window_id` |

Use `get_displays` to understand the display configuration:
```json
{
  "displays": [{
    "id": 1,
    "is_main": true,
    "bounds": { "x": 0, "y": 0, "width": 3008, "height": 1692 },
    "backing_scale_factor": 2.0,
    "pixel_width": 6016,
    "pixel_height": 3384
  }]
}
```

## Example Usage

### With Native Input (any app)

**Option A: Using OCR (recommended for text elements)**
```
User: Click the Submit button in the app

Claude: [calls find_text with text="Submit"]
        [receives: {"text": "Submit", "x": 450, "y": 320}]
        [calls click with x=450, y=320]
```

**Option B: Using screenshot metadata (for any visual element)**
```
User: Click the icon next to Settings

Claude: [calls take_screenshot]
        [receives image + metadata: {"screenshot_origin_x": 0, "screenshot_origin_y": 0, "screenshot_scale": 2.0}]
        [visually identifies icon at pixel (300, 150) in the image]
        [calls click with screenshot_x=300, screenshot_y=150, screenshot_origin_x=0, screenshot_origin_y=0, screenshot_scale=2.0]
```

### With AppDebugKit (embedded app)

```
User: Click the Submit button in the app

Claude: [calls app_connect with url="ws://127.0.0.1:9222"]
        [calls app_query with selector="[title=Submit]"]
        [receives element_id="view-42"]
        [calls app_click with element_id="view-42"]
```

## Architecture

```
┌─────────────────┐     JSON-RPC 2.0      ┌──────────────────┐
│  Claude/Client  │ ◄──────────────────► │  native-devtools │
│  (with vision)  │      stdio           │     MCP Server   │
└─────────────────┘                       └────────┬─────────┘
                                                   │
                          ┌────────────────────────┼────────────────────────┐
                          │                        │                        │
                          ▼                        ▼                        ▼
                   ┌─────────────┐          ┌─────────────┐          ┌─────────────┐
                   │  AppDebugKit │          │   Native    │          │   System    │
                   │  (WebSocket) │          │   Input     │          │   APIs      │
                   │              │          │             │          │             │
                   │ Element-level│          │ macOS:      │          │ Screenshots │
                   │ interaction  │          │  CGEvent    │          │ Window enum │
                   └─────────────┘          │ Windows:    │          │ OCR         │
                         │                  │  SendInput  │          └─────────────┘
                         │                  └─────────────┘
                         ▼                        │
                   ┌─────────────┐                ▼
                   │ Apps with   │          ┌─────────────┐
                   │ AppDebugKit │          │  Any app    │
                   └─────────────┘          │ (egui, etc) │
                                            └─────────────┘

Platform Backends:
┌──────────────────────────────────────────────────────────────────────┐
│ macOS: CGEvent, screencapture, Vision OCR, CGWindowList              │
│ Windows: SendInput, GDI BitBlt, Windows.Media.Ocr, EnumWindows       │
└──────────────────────────────────────────────────────────────────────┘
```

## License

MIT
