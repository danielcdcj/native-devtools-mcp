# OCR Fallback and Element Inspection

Use this flow when `find_text` does not give you a usable result.

## Typical reasons

- The app exposes poor or unusual accessibility labels
- The visible text does not match the accessibility name
- The app restricts its accessibility tree
- You want to inspect what is actually under a specific cursor position

## Fallback 1: OCR from `take_screenshot`

Take a screenshot with OCR enabled:

```json
{
  "tool": "take_screenshot",
  "arguments": {
    "app_name": "Signal",
    "include_ocr": true
  }
}
```

The metadata includes:

- `screenshot_origin_x`
- `screenshot_origin_y`
- `screenshot_scale`
- `screenshot_pixel_width`
- `screenshot_pixel_height`
- `screenshot_id`

The OCR text block includes clickable coordinates for detected text. If OCR shows the target text directly, click those screen coordinates.

## Fallback 2: Inspect a point with `element_at_point`

If you already know roughly where the UI element is, inspect that screen position:

```json
{
  "tool": "element_at_point",
  "arguments": {
    "x": 500,
    "y": 300,
    "app_name": "Signal"
  }
}
```

Example result:

```json
{
  "role": "AXButton",
  "name": "Save",
  "label": "Save document",
  "bounds": { "x": 480, "y": 290, "width": 40, "height": 20 },
  "app_name": "Signal"
}
```

## Recommended sequence

1. Try `find_text`
2. If that fails, use `take_screenshot(include_ocr=true)`
3. If the app has accessibility quirks, inspect likely coordinates with `element_at_point`
4. Use `click` only after you have a grounded target

## Notes

- Privacy-focused Electron apps may expose only container elements through accessibility.
- In that case, OCR from `take_screenshot` is usually the better fallback.
