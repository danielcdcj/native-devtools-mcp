# Native App Click Flow

Use this pattern when you want to click a text-labeled element in a desktop app.

> **macOS native apps:** For AppKit / SwiftUI apps, the [Native App AX Dispatch Flow](./native-app-ax-dispatch-flow.md) is preferred — it's element-precise, doesn't move the mouse, and doesn't steal focus. Use this `find_text` + `click` recipe on Windows, for Electron apps, or when the AX tree doesn't expose the target.

## When to use it

- The target is a macOS or Windows desktop app
- The target element has text such as `Save`, `Submit`, `OK`, or `Settings`
- You want the most reliable path before falling back to OCR or template matching
- On macOS: the app's AX tree is restricted (e.g. Signal, some Electron apps) or the AX dispatch tools returned `not_dispatchable`

## Preferred flow

1. Focus the target app window
2. Search for the element with `find_text`
3. Click the returned coordinates
4. Verify the result with `take_screenshot`

## Example

```json
{ "tool": "focus_window", "arguments": { "app_name": "TextEdit" } }
```

```json
{ "tool": "find_text", "arguments": { "text": "Save", "app_name": "TextEdit" } }
```

Example result:

```json
[
  {
    "text": "Save",
    "x": 500,
    "y": 300,
    "confidence": 1.0,
    "bounds": { "x": 480, "y": 290, "width": 40, "height": 20 }
  }
]
```

Click the returned coordinates:

```json
{ "tool": "click", "arguments": { "x": 500, "y": 300 } }
```

Verify:

```json
{ "tool": "take_screenshot", "arguments": { "app_name": "TextEdit", "include_ocr": true } }
```

## Why this is the preferred path

- `find_text` uses the accessibility tree first
- It gives direct screen coordinates
- It is usually more reliable than guessing from pixels alone

## If `find_text` fails

Move to one of these fallbacks:

- [Native App AX Dispatch Flow (macOS)](./native-app-ax-dispatch-flow.md) — if `find_text` couldn't name the element but the AX tree does
- [OCR Fallback and Element Inspection](./ocr-fallback-and-element-inspection.md)
- [Template Matching Flow](./template-matching-flow.md)
