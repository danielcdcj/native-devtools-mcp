# Template Matching Flow

Use this flow when the target is not text: icons, glyphs, toggles, shapes, or custom controls.

## When to use it

- OCR cannot identify the target
- `find_text` is not applicable
- You have a reference image for the element you want to click

## Flow

1. Take a screenshot of the app window
2. Load the reference image with `load_image`
3. Search the screenshot with `find_image`
4. Click the returned `screen_x` and `screen_y`

## Example

Take a screenshot:

```json
{
  "tool": "take_screenshot",
  "arguments": {
    "app_name": "MyApp"
  }
}
```

The screenshot metadata includes `screenshot_id`. Use that in the next step.

Load a template image:

```json
{
  "tool": "load_image",
  "arguments": {
    "path": "/path/to/icon.png"
  }
}
```

Example result:

```json
{
  "image_id": "image-0",
  "width": 64,
  "height": 64,
  "channels": 4,
  "mime": "image/png"
}
```

Find the image:

```json
{
  "tool": "find_image",
  "arguments": {
    "screenshot_id": "screenshot-0",
    "template_id": "image-0"
  }
}
```

Example result:

```json
{
  "matches": [
    {
      "score": 0.95,
      "center": { "x": 132, "y": 232 },
      "screen_x": 166,
      "screen_y": 216
    }
  ]
}
```

Click the match:

```json
{
  "tool": "click",
  "arguments": {
    "x": 166,
    "y": 216
  }
}
```

## Tips

- Start with the default `fast` mode
- Use `accurate` only when you need a wider or more precise search
- Keep the template image tightly cropped around the visual element
