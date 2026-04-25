# End-to-End Desktop Flow

Use this recipe for the common pattern:

- open an app
- find a text-labeled field or button
- click it
- type text
- verify the result

This is a reusable desktop pattern for macOS and Windows. Replace the app name, target text, and input text with values that match your app.

> **On macOS, prefer the AX dispatch path for native apps.** Replace `find_text` + `click` with `take_ax_snapshot` + `ax_click`, and replace `click` + `type_text` with `ax_set_value` when the target is an `AXTextField` / `AXSearchField`. The AX path doesn't move the mouse or steal focus. See the [Native App AX Dispatch Flow](./native-app-ax-dispatch-flow.md). Keep this `find_text` + `click` + `type_text` recipe for Windows, Electron apps, or when the AX tree doesn't expose the target.

## Goal

Open a desktop app, place focus into a text field or search box, enter text, and confirm the UI changed.

## Flow

1. Launch or activate the app
2. Focus its window
3. Find the target text with `find_text`
4. Click the returned coordinates
5. Type into the focused field
6. Verify with `take_screenshot`

## Example

Launch or activate the app:

```json
{
  "tool": "launch_app",
  "arguments": {
    "app_name": "MyApp"
  }
}
```

Bring it to the front:

```json
{
  "tool": "focus_window",
  "arguments": {
    "app_name": "MyApp"
  }
}
```

Find the field, button, or visible label you want to interact with:

```json
{
  "tool": "find_text",
  "arguments": {
    "text": "Search",
    "app_name": "MyApp"
  }
}
```

Example result:

```json
[
  {
    "text": "Search",
    "x": 420,
    "y": 180,
    "confidence": 1.0,
    "bounds": { "x": 360, "y": 160, "width": 120, "height": 32 }
  }
]
```

Click the returned coordinates:

```json
{
  "tool": "click",
  "arguments": {
    "x": 420,
    "y": 180
  }
}
```

Type text into the focused element:

```json
{
  "tool": "type_text",
  "arguments": {
    "text": "hello from native-devtools-mcp"
  }
}
```

Verify the result:

```json
{
  "tool": "take_screenshot",
  "arguments": {
    "app_name": "MyApp",
    "include_ocr": true
  }
}
```

## What to look for during verification

- The text you typed appears in the OCR output
- The app remains focused and visible
- The next expected UI state is now on screen

## If the target is not found

Try one of these:

- use [OCR Fallback and Element Inspection](./ocr-fallback-and-element-inspection.md)
- use [Template Matching Flow](./template-matching-flow.md) for icons and custom controls

## Notes

- `launch_app` brings an app to the front when it is already running and no args are provided
- Always verify after typing because simulated input can go to the wrong window if focus changed
