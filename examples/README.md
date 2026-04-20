# Recipes and Examples

These examples are grounded in the current tool names and setup paths used by `native-devtools-mcp`.

## Setup

- [Claude Desktop Setup](./claude-desktop-setup.md)
- [Claude Code Setup](./claude-code-setup.md)
- [Cursor Setup](./cursor-setup.md)

## Desktop automation

- [End-to-End Desktop Flow](./end-to-end-desktop-flow.md)
- [Native App AX Dispatch Flow (macOS)](./native-app-ax-dispatch-flow.md) — preferred for native macOS apps
- [Native App Click Flow](./native-app-click-flow.md)
- [OCR Fallback and Element Inspection](./ocr-fallback-and-element-inspection.md)
- [Template Matching Flow](./template-matching-flow.md)

## Android

- [Android Quickstart](./android-quickstart.md)

## Notes

- For desktop apps, use tools without a prefix: `click`, `find_text`, `take_screenshot`, `type_text`, `focus_window`, and related tools.
- For native macOS apps, prefer the AX dispatch tools: `take_ax_snapshot` → `ax_click` / `ax_set_value` / `ax_select`. These don't move the cursor or steal focus.
- For Android devices, use tools with the `android_` prefix.
- Prefer `find_text` for text elements, then fall back to OCR or `find_image` when needed.
