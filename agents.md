# Agent Context: native-devtools-mcp

Purpose: MCP server for native desktop app automation using screenshots, OCR, window enumeration, and input injection. Optional AppDebugKit enables element-level control.

## Capabilities Matrix

| Intent | Tools | Outputs |
|--------|-------|---------|
| Capture screen or window | `take_screenshot` | base64 PNG, screenshot metadata (origin, scale), optional OCR text |
| Find text and click it | `find_text` → `click` | coordinates, click action |
| List and focus windows | `list_windows` → `focus_window` | window list, focus action |
| Element-level UI control | `app_connect` → `app_query` → `app_click` | element IDs, click action |

## Intent Spec (JSON)

```json
{
  "intents": [
    {
      "name": "capture_screenshot",
      "tools": ["take_screenshot"],
      "inputs": {
        "scope": { "type": "string", "enum": ["screen", "window", "region"] },
        "window_id": { "type": "number", "optional": true },
        "region": { "type": "object", "optional": true },
        "include_ocr": { "type": "boolean", "default": true }
      },
      "outputs": {
        "image_base64": { "type": "string" },
        "metadata": { "type": "object", "optional": true },
        "ocr": { "type": "array", "optional": true }
      }
    },
    {
      "name": "find_text_and_click",
      "tools": ["find_text", "click"],
      "inputs": {
        "query": { "type": "string" },
        "window_id": { "type": "number", "optional": true }
      },
      "outputs": {
        "matches": { "type": "array" },
        "clicked": { "type": "boolean" }
      }
    },
    {
      "name": "list_and_focus_window",
      "tools": ["list_windows", "focus_window"],
      "inputs": {
        "app_name": { "type": "string", "optional": true }
      },
      "outputs": {
        "windows": { "type": "array" },
        "focused": { "type": "boolean" }
      }
    },
    {
      "name": "element_level_interaction",
      "tools": ["app_connect", "app_query", "app_click", "app_type"],
      "inputs": {
        "selector": { "type": "string" },
        "element_id": { "type": "string", "optional": true },
        "text": { "type": "string", "optional": true }
      },
      "outputs": {
        "element": { "type": "object" },
        "ok": { "type": "boolean" }
      }
    }
  ]
}
```

## Prompt -> Tool -> Output Mappings

| User prompt | Tool sequence | Expected output |
|-------------|---------------|-----------------|
| "Take a screenshot of the Settings window" | `list_windows` → `take_screenshot(window_id)` | base64 PNG, metadata, OCR text |
| "Click the OK button" | `take_screenshot` → (vision) → `click(x,y)` | click action |
| "Find text 'Submit' and click it" | `find_text(query)` → `click(x,y)` | coordinates, click action |
| "Click the Save button in the AppDebugKit app" | `app_connect` → `app_query("[title=Save]")` → `app_click(element_id)` | element ID, click action |

## Coordinate Usage Guide

When clicking, choose the correct coordinate format based on how you obtained the target position:

| Coordinate source | Click parameters | Example |
|-------------------|------------------|---------|
| `find_text` result | `x`, `y` (direct) | `{"x": 450, "y": 320}` |
| `take_screenshot` OCR annotation | `x`, `y` (direct) | `{"x": 450, "y": 320}` |
| Visual inspection of screenshot | `screenshot_x/y` + metadata | `{"screenshot_x": 200, "screenshot_y": 100, "screenshot_origin_x": 0, "screenshot_origin_y": 0, "screenshot_scale": 2.0}` |

**Key distinction:**
- **OCR coordinates** (from `find_text` or `take_screenshot` OCR) are already screen-absolute → use `x`, `y` directly
- **Screenshot pixel coordinates** (from visually inspecting the image) need transformation → use `screenshot_x/y` with the metadata returned by `take_screenshot`

## Operational Notes

- Requires macOS Screen Recording permission for screenshots and Accessibility permission for input.
- OCR depends on Tesseract when `include_ocr: true`.
- Keep the target window focused during automation to avoid misdirected input.
