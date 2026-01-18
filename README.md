# native-devtools-mcp

A Model Context Protocol (MCP) server for testing native desktop applications, similar to how Chrome DevTools enables web UI testing.

> **100% Local & Private** - All processing happens on your machine. No data is sent to external servers. Screenshots, UI interactions, and app data never leave your device.

## Platform Support

| Platform | Status |
|----------|--------|
| **macOS** | Supported |
| **Windows** | Planned |
| **Linux** | Planned |

> Windows and Linux support will be added in future releases with platform-specific backends.

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

### 2. CGEvent (`click`, `type_text`, etc.) - Universal Compatibility

For **any application** regardless of framework:
- **Screen coordinate targeting** - Click at (x, y) positions
- **Works with any UI framework** - egui, Electron, Qt, games, anything
- **No app modification required** - Just needs Accessibility permission

**Best for:** Third-party apps, egui/Electron/Qt apps, when AppDebugKit isn't available.

```
take_screenshot → (analyze visually) → click(x=500, y=300)
```

### Why Two Approaches?

The LLM needs to choose the right approach based on what it observes:

| Scenario | Recommended Approach |
|----------|---------------------|
| App with AppDebugKit embedded | `app_*` tools - reliable element IDs |
| egui/Electron/Qt app | CGEvent tools - coordinate-based |
| Unknown app | Try `app_connect`, fall back to CGEvent |
| App with poor view hierarchy | CGEvent even if AppDebugKit connected |

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

# Binary location
./target/release/native-devtools-mcp
```

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

#### 3. Tesseract OCR (required for `find_text`, and for `take_screenshot` OCR)

The `find_text` tool uses OCR to locate text on screen and return clickable coordinates. The `take_screenshot` tool also runs OCR by default (`include_ocr: true`) to return text annotations. This is the **recommended way** to interact with apps when AppDebugKit is not available.

```bash
brew install tesseract
```

### Important Notes

- **Grant permissions to the host app** (VS Code, Terminal), not to the MCP server binary itself
- **Restart is required** - Permissions don't take effect until you fully quit and reopen the app
- **No popup appears** - macOS won't prompt you; it silently fails if permissions are missing
- If you see `could not create image from display`, you need Screen Recording permission
- If clicks don't work, you need Accessibility permission

### Privacy & Security

All data stays on your machine:
- Screenshots are captured locally and sent directly to Claude via the MCP protocol
- No data is uploaded to external servers
- The MCP server runs entirely offline
- Source code is open for audit

## MCP Configuration

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

Or if you built from source:

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
| `take_screenshot` | Capture screen, window, or region (base64 PNG). Includes OCR text annotations by default (`include_ocr: true`). |
| `list_windows` | List visible windows with IDs, titles, bounds |
| `list_apps` | List running applications |
| `focus_window` | Bring window/app to front |
| `get_displays` | Get display info (bounds, scale factors) for coordinate conversion |
| `find_text` | Find text on screen using OCR; returns screen coordinates for clicking |

### CGEvent Input Tools (work with any app, require Accessibility permission)

| Tool | Description |
|------|-------------|
| `click` | Click at screen/window/screenshot coordinates |
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

## How Screenshots and Clicking Work (macOS)

- **Screenshots** are captured via the system `screencapture` utility (`-x` silent, `-C` include cursor, `-R` region, `-l` window with `-o` to exclude shadow), written to a temp PNG, and returned as base64. The backing scale factor is tracked for coordinate conversion. Window screenshots exclude shadows so that pixel coordinates align exactly with `CGWindowBounds`, and OCR coordinates are automatically offset into screen space.
- **Clicks/inputs** use CoreGraphics CGEvent injection (HID event tap). This requires Accessibility permission and works across AppKit, SwiftUI, Electron, egui, etc. Window-relative or screenshot-pixel coordinates are converted to screen coordinates using window bounds and display scale.

## Coordinate Systems and Display Scaling

The `click` tool supports three coordinate input methods to handle macOS display scaling:

```json
// Direct screen coordinates
{ "x": 500, "y": 300 }

// Window-relative (converted using window bounds)
{ "window_x": 100, "window_y": 50, "window_id": 1234 }

// Screenshot pixels (converted using backing scale factor)
{ "screenshot_x": 200, "screenshot_y": 100, "screenshot_window_id": 1234 }
```

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

### With CGEvent (any app)

```
User: Click the Submit button in the app

Claude: [calls take_screenshot]
        [analyzes screenshot, finds Submit button at approximately x=450, y=320]
        [calls click with x=450, y=320]
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
                   │  AppDebugKit │          │   CGEvent   │          │   System    │
                   │  (WebSocket) │          │   Input     │          │   APIs      │
                   │              │          │             │          │             │
                   │ Element-level│          │ Coordinate  │          │ Screenshots │
                   │ interaction  │          │ based input │          │ Window enum │
                   └─────────────┘          └─────────────┘          └─────────────┘
                         │                        │
                         ▼                        ▼
                   ┌─────────────┐          ┌─────────────┐
                   │ Apps with   │          │  Any app    │
                   │ AppDebugKit │          │ (egui, etc) │
                   └─────────────┘          └─────────────┘
```

## License

MIT
