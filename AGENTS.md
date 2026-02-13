# Agent Context: native-devtools-mcp

**About:** This is the AGENTS.md for **native-devtools-mcp**, an MCP (Model Context Protocol) server that enables **computer use** / **desktop automation** on macOS and Windows: screenshots, OCR, mouse/keyboard input, and window management.

**Search keywords:** MCP, Model Context Protocol, computer use, desktop automation, UI automation, RPA, screenshots, OCR, screen reading, mouse, keyboard, macOS, Windows, native-devtools-mcp.

**Role:** You are an agent equipped with "Computer Use" capabilities. You can see the screen, type, move the mouse, and interact with native desktop applications.

**Constraint:** You are operating a real machine. Actions are permanent. Ensure you verify the state of the screen before and after actions.

## 🧠 Core Reasoning Loop

For robust automation, follow this "Visual Feedback Loop":

1.  **OBSERVE:** Call `take_screenshot(app_name="TargetApp")` to see the current state.
2.  **LOCATE:** Analyze the image or use the OCR summary text in the response to find coordinates.
3.  **ACT:** Call `click()`, `type_text()`, or `scroll()` using those coordinates.
4.  **VERIFY:** Call `take_screenshot` again to confirm the action had the intended effect.

---

## 🗺️ Capabilities Matrix (Strategy Guide)

Use this table to choose the right tool sequence for the user's goal.

| User Goal | Tool Sequence | Why? |
|-----------|---------------|------|
| "Click the 'Submit' button" | `find_text(text="Submit")` → `click(x, y)` | Fastest. No visual analysis needed if text is known. |
| "Click the red icon" | `take_screenshot()` → (Analyze Image) → `click(screenshot_x=..., screenshot_y=..., screenshot_origin_x=..., screenshot_origin_y=..., screenshot_scale=...)` | Visual features require full screenshot analysis. |
| "Type into the search bar" | `find_text(text="Search")` → `click(x, y)` → `type_text("hello")` | Must click to focus before typing. |
| "Scroll down" | `scroll(x=500, y=500, delta_y=200)` | Positive `delta_y` scrolls down. |
| "Find an open window" | `list_windows()` → `focus_window(window_id=...)` | Don't guess window names; list them first. |

---

## 🛠️ Tool Definitions & Schemas

### 1. Vision & Perception (The "Eyes")

#### `take_screenshot`
Captures pixel data and layout.
*   **Inputs:**
    *   `mode` (string, default `"window"`): `"screen"`, `"window"`, or `"region"`.
    *   `app_name` (string, optional): Capture this app's window (for mode `"window"`).
    *   `window_id` (number, optional): Window ID (for mode `"window"`).
    *   `x`, `y`, `width`, `height` (numbers): Region bounds (for mode `"region"`).
    *   `include_ocr` (boolean, default `true`): Include OCR summary text with coordinates.
*   **Returns (content list):**
    ```json
    [
      { "type": "image", "mime": "image/jpeg", "data": "..." },
      { "type": "text", "text": "{ \"screenshot_origin_x\": 0, \"screenshot_origin_y\": 0, \"screenshot_scale\": 2.0, \"screenshot_window_id\": 1234, \"screenshot_pixel_width\": 1920, \"screenshot_pixel_height\": 1080 }" },
      { "type": "text", "text": "## OCR Text Detected (click coordinates)\n- \"File\" at (10, 10) bounds: {x: 0, y: 0, w: 50, h: 20}" }
    ]
    ```
    *   **Metadata fields:**
        *   `screenshot_origin_x`, `screenshot_origin_y`: Screen-space origin of the screenshot (top-left corner), in points.
        *   `screenshot_scale`: Display scale factor (e.g., 2.0 for Retina).
        *   `screenshot_window_id`: Window ID (only for mode `"window"`). Present even when using `app_name`.
        *   `screenshot_pixel_width`, `screenshot_pixel_height`: Actual pixel dimensions of the captured image.

#### `find_text`
Fast-path to get coordinates without image analysis.
*   **Inputs:** `text` (string, case-insensitive substring match against accessibility element names, then OCR), `app_name` (string, optional), `window_id` (number, optional), `display_id` (number, optional).
*   **Returns (JSON array):**
    ```json
    [
      { "text": "Save", "x": 500, "y": 300, "confidence": 1.0, "bounds": { "x": 480, "y": 290, "width": 40, "height": 20 } }
    ]
    ```
*   **Platform behavior:**
    *   **Both platforms:** Uses the **platform accessibility API** as the primary mechanism — searches the accessibility tree for elements by name. This gives precise element-level coordinates (`confidence: 1.0`). Falls back to OCR automatically if accessibility finds no matches.
    *   **macOS:** Accessibility API (primary), Vision OCR (fallback). Matches against element title, value, and description. Note: accessibility results use semantic names (e.g., "All Clear" instead of "AC", "Subtract" instead of "−"), so search by meaning rather than displayed symbol.
    *   **Windows:** UI Automation (primary), WinRT OCR (fallback). Matches against element Name property only.

### 2. Input & Interaction (The "Hands")

#### `click`
Simulates a mouse click.
*   **Inputs:**
    *   **Method A (Screen Absolute):** `x` (number), `y` (number). Use with `find_text` results.
    *   **Method B (Window Relative):** `window_x`, `window_y`, `window_id`.
    *   **Method C (Screenshot Relative):** `screenshot_x`, `screenshot_y`, `screenshot_origin_x`, `screenshot_origin_y`, `screenshot_scale`. Use with `take_screenshot` visual analysis.
    *   `button`: "left" (default), "right", "center".
    *   `click_count`: 1 (default), 2 (double-click).

#### `type_text`
Types text at the *current* cursor position.
*   **Inputs:** `text` (string).
*   **Warning:** Always `click()` the input field first to ensure focus!

#### `scroll`
Scrolls at a specific screen position.
*   **Inputs:** `x` (number), `y` (number), `delta_y` (integer), `delta_x` (integer, optional).
*   **Direction:** Positive `delta_y` scrolls down; negative scrolls up.

### 3. Window Management

*   `list_windows`: Returns array of `{ id, title, bounds, app_name }`.
*   `focus_window`: Accepts `{ window_id: 123 }`, `{ app_name: "Code" }`, or `{ pid: 999 }`.

---

## 📐 Coordinate Systems & Best Practices

**CRITICAL:** There are two ways to target clicks. Choose ONE based on your data source.

### Method A: Absolute Screen Coordinates (Recommended)
Use this when you have data from `find_text` OR `take_screenshot` (OCR results).
*   **Source:** `find_text` returns `{ "x": 500, "y": 300 }`.
*   **Action:** `click(x=500, y=300)`.
*   **Why:** These are already global screen coordinates.

### Method B: Relative Screenshot Coordinates
Use this when you (the model) look at the *image* from `take_screenshot` and estimate positions (e.g., "The icon is at 50% width").
*   **Source:** `take_screenshot` returns metadata `{ "screenshot_origin_x": 100, "screenshot_origin_y": 100, "screenshot_scale": 2.0, "screenshot_pixel_width": 1920, "screenshot_pixel_height": 1080 }`.
*   **Your Vision:** You see a button at pixel `(x=50, y=50)` inside the image.
*   **Action:** `click(screenshot_x=50, screenshot_y=50, screenshot_origin_x=100, screenshot_origin_y=100, screenshot_scale=2.0)`.
*   **Why:** The tool handles the math to convert image-pixels to screen-pixels.

**Manual conversion (for tools that only accept screen coordinates, e.g. `drag`):**
*   `screen_x = screenshot_origin_x + (screenshot_x / screenshot_scale)`
*   `screen_y = screenshot_origin_y + (screenshot_y / screenshot_scale)`

---

## ⚡ Intent Examples (Chain of Thought)

### "Click the 'Save' button in Notepad"
1.  **Thought:** I need to find the text "Save" in the app "Notepad".
2.  **Call:** `focus_window(app_name="Notepad")`
3.  **Call:** `find_text(text="Save")` -> Returns `[{"text":"Save","x":200,"y":400,...}]`
4.  **Call:** `click(x=200, y=400)`

### "Draw a circle in Paint"
1.  **Thought:** Text search won't work for a canvas. I need to see the screen.
2.  **Call:** `take_screenshot(app_name="Paint")`
3.  **Analysis:** I see the canvas center at pixel (500, 500) in the image.
4.  **Compute:** `start_x = screenshot_origin_x + 500 / screenshot_scale`, `start_y = screenshot_origin_y + 500 / screenshot_scale`
5.  **Call:** `drag(start_x=..., start_y=..., end_x=..., end_y=...)`

### "Copy text from this window"
1.  **Thought:** I can read text directly from the screenshot OCR data without using the clipboard.
2.  **Call:** `take_screenshot(include_ocr=true)`
3.  **Action:** Read the OCR summary text in the response (lines include clickable coordinates).

---

## 🖼️ Template Matching (Advanced Vision)

For finding non-text UI elements like icons, shapes, or specific visual patterns, use the `find_image` tool with template matching.

### `load_image`
Load an image from a local file path and cache it for use with `find_image`.
*   **Inputs:**
    *   `path` (string, required): Local filesystem path to the image file.
    *   `id_prefix` (string, optional): Prefix for the generated ID (e.g., "template", "mask").
    *   `max_width`, `max_height` (integer, optional): Downscale constraints (maintains aspect ratio).
    *   `as_mask` (boolean, default `false`): Convert to single-channel grayscale mask.
    *   `return_base64` (boolean, default `false`): Include base64-encoded image data in response.
*   **Returns (JSON):**
    ```json
    {
      "image_id": "template-0",
      "width": 64,
      "height": 64,
      "channels": 4,
      "mime": "image/png",
      "sha256": "abc123..."
    }
    ```

### `find_image`
Find a template image within a screenshot using template matching. Returns precise click coordinates.
*   **Inputs:**
    *   `screenshot_id` (string, optional): Screenshot ID from `take_screenshot` (preferred).
    *   `screenshot_image_base64` (string, optional): Base64-encoded screenshot (if no screenshot_id).
    *   `template_id` (string, optional): Image ID from `load_image` (preferred).
    *   `template_image_base64` (string, optional): Base64-encoded template (if no template_id).
    *   `mask_id` (string, optional): Image ID for the mask (from `load_image`).
    *   `mask_image_base64` (string, optional): Base64-encoded mask (white=match, black=ignore).
    *   `mode` (string, default `"fast"`): `"fast"` or `"accurate"`. Fast uses downscaling/early-exit for speed; accurate uses full-res, wider scales, smaller stride.
    *   `threshold` (number, optional): Minimum match score 0.0-1.0.
    *   `max_results` (integer, optional): Maximum matches to return.
    *   `scales` (object, optional): Scale search range `{min, max, step}`.
    *   `rotations` (array, optional): Rotations to try in degrees (only 0, 90, 180, 270 supported).
*   **Returns (JSON):**
    ```json
    {
      "matches": [
        {
          "score": 0.95,
          "bbox": {"x": 100, "y": 200, "w": 64, "h": 64},
          "center": {"x": 132, "y": 232},
          "scale": 1.0,
          "rotation": 0,
          "screen_x": 166,
          "screen_y": 216
        }
      ]
    }
    ```

### Template Matching Example Flow
```
1. take_screenshot(app_name="MyApp")      → screenshot_id: "screenshot-0"
2. load_image(path="/path/to/icon.png")   → image_id: "image-0"
3. find_image(screenshot_id="screenshot-0", template_id="image-0")
   → matches: [{screen_x: 150, screen_y: 200, ...}]
4. click(x=150, y=200)
```
