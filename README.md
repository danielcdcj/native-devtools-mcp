# macos-devtools-mcp

A Model Context Protocol (MCP) server for testing native macOS applications, similar to how Chrome DevTools enables web UI testing.

## Overview

This MCP server enables LLM-driven testing of native macOS apps by providing:
- **Screenshots** - Capture full screen, windows, or regions
- **Input simulation** - Click, type, scroll, drag via Core Graphics events
- **Window/app enumeration** - List and focus windows and applications

The LLM interprets screenshots visually to decide actions—no OCR or accessibility tree required.

## Installation

```bash
# Build
cargo build --release

# Binary location
./target/release/macos-devtools-mcp
```

## Required Permissions

Grant these in **System Settings > Privacy & Security**:
- **Screen Recording** - For screenshots
- **Accessibility** - For input simulation

## MCP Configuration

Add to your Claude Code MCP config (`~/.claude/claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "macos-devtools": {
      "command": "/path/to/macos-devtools-mcp"
    }
  }
}
```

## Tools

| Tool | Description |
|------|-------------|
| `take_screenshot` | Capture screen, window, or region (base64 PNG) |
| `list_windows` | List visible windows with IDs, titles, bounds |
| `list_apps` | List running applications |
| `focus_window` | Bring window/app to front |
| `click` | Click at (x, y) - left/right/middle, single/double |
| `type_text` | Type text at cursor |
| `press_key` | Press key combo (e.g., "Cmd+C", "Enter") |
| `scroll` | Scroll at position |
| `drag` | Drag from point to point |
| `move_mouse` | Move cursor (for hover) |
| `wait` | Wait milliseconds |

## Example Usage

```
User: Take a screenshot and click on the Calculator app icon in the dock

Claude: [calls take_screenshot]
        [analyzes screenshot, finds Calculator at x=500, y=1050]
        [calls click with x=500, y=500]
```

## Architecture

```
┌─────────────────┐     JSON-RPC 2.0      ┌──────────────────┐
│  Claude/Client  │ ◄──────────────────► │  macos-devtools  │
│  (with vision)  │      stdio           │     MCP Server   │
└─────────────────┘                       └────────┬─────────┘
                                                   │
                               ┌───────────────────┼───────────────────┐
                               ▼                   ▼                   ▼
                        ┌────────────┐      ┌────────────┐      ┌────────────┐
                        │screencapture│      │Core Graphics│      │ NSWorkspace│
                        │   (CLI)    │      │   Events   │      │   (objc)   │
                        └────────────┘      └────────────┘      └────────────┘
```

## License

MIT
