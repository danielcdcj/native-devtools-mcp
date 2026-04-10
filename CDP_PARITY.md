# CDP Feature Parity: native-devtools-mcp vs chrome-devtools-mcp

This document tracks feature parity between our CDP tools and [chrome-devtools-mcp](https://github.com/anthropics/chrome-devtools-mcp).

## Tool Comparison

| chrome-devtools-mcp | native-devtools-mcp | Notes |
|---|---|---|
| `take_snapshot` | `cdp_take_ax_snapshot` | Accessibility tree snapshot with a-prefixed UIDs |
| — | `cdp_take_dom_snapshot` | DOM-native snapshot of interactive elements with d-prefixed UIDs |
| — | `cdp_find_elements` | Search live DOM for interactive elements by text query |
| `click` | `cdp_click` | Click by UID, supports double-click |
| `hover` | `cdp_hover` | Hover by UID |
| `fill` | `cdp_fill` | Fill input/textarea/select by UID |
| `press_key` | `cdp_press_key` | Key combos (e.g., `Control+A`) |
| `handle_dialog` | `cdp_handle_dialog` | Accept/dismiss JS dialogs |
| `navigate_page` | `cdp_navigate` | URL, back, forward, reload |
| `new_page` | `cdp_new_page` | Create new tab |
| `close_page` | `cdp_close_page` | Close tab by index |
| `wait_for` | `cdp_wait_for` | Poll snapshot for text |
| `list_pages` | `cdp_list_pages` | List open tabs |
| `select_page` | `cdp_select_page` | Switch active tab |
| `evaluate_script` | `cdp_evaluate_script` | Run JS with optional UID args |
| `take_screenshot` | `take_screenshot` | Native screenshot (not CDP) |
| `type_text` | `cdp_press_key` | Use press_key for individual keys |
| `drag` | — | Not implemented |
| `fill_form` | — | Not implemented (use cdp_fill per field) |
| `upload_file` | — | Not implemented |
| `emulate` | — | Not implemented (device emulation) |
| `resize_page` | — | Not implemented |
| `lighthouse_audit` | — | Not implemented |
| `take_memory_snapshot` | — | Not implemented |
| `performance_start_trace` | — | Not implemented |
| `performance_stop_trace` | — | Not implemented |
| `performance_analyze_insight` | — | Not implemented |
| `get_console_message` | — | Not implemented |
| `list_console_messages` | — | Not implemented |
| `get_network_request` | — | Not implemented |
| `list_network_requests` | — | Not implemented |
| — | `cdp_connect` | Connect via debugging port (chrome-devtools-mcp auto-connects) |
| — | `cdp_disconnect` | Disconnect from browser |

## Not Implemented (and why)

### Low priority — niche or covered by other tools

- **drag**: Uncommon interaction pattern. Can be done via `cdp_evaluate_script` with drag events.
- **fill_form**: Convenience wrapper — use `cdp_fill` for each field individually.
- **upload_file**: Requires `DOM.setFileInputFiles` — niche use case.
- **type_text**: `cdp_press_key` covers key-by-key input; `cdp_fill` covers bulk text entry.

### Performance/debugging — out of scope

- **lighthouse_audit**, **performance_\***, **take_memory_snapshot**: Performance profiling tools. Out of scope for a general automation MCP.
- **emulate**, **resize_page**: Device emulation. Can be done via `cdp_evaluate_script` if needed.

### Console/network monitoring

- **get_console_message**, **list_console_messages**: Requires subscribing to `Runtime.consoleAPICalled` events. Potential future addition.
- **get_network_request**, **list_network_requests**: Requires subscribing to `Network.requestWillBeSent` events. Potential future addition.

## Our total: 15 CDP tools (1 always visible + 14 gated behind connection)
