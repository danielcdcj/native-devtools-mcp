# Agent Context: native-devtools-mcp

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
*   **Inputs:** `text` (string), `display_id` (number, optional).
*   **Returns (JSON array):**
    ```json
    [
      { "text": "Save", "x": 500, "y": 300, "confidence": 0.94, "bounds": { "x": 480, "y": 290, "width": 40, "height": 20 } }
    ]
    ```

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
