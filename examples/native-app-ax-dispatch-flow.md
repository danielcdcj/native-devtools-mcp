# Native App AX Dispatch Flow (macOS)

Use this pattern when you want to automate a native macOS app (AppKit / SwiftUI) — System Settings, Finder, Mail, Xcode, Notes, Preview, most App Store apps — without moving the mouse or stealing focus.

## When to use it

- The target is a native macOS app with a rich Accessibility tree
- You want automation that composes with background work (no cursor movement, no focus steal)
- You are pressing buttons, writing text fields, or selecting sidebar / table rows

> This is the **preferred** path for native macOS apps. For Electron, Chrome, or apps with a restricted AX tree, fall back to the [Native App Click Flow](./native-app-click-flow.md) or [OCR Fallback](./ocr-fallback-and-element-inspection.md).

## The three AX primitives

| Primitive       | AX mechanism                | Best for                                                                 |
|-----------------|-----------------------------|--------------------------------------------------------------------------|
| `ax_click`      | `AXPress` action            | Buttons, menu items, checkboxes, toolbar items                           |
| `ax_set_value`  | write `kAXValueAttribute`   | Text fields, search fields, stepper-backed inputs                        |
| `ax_select`     | write `AXSelectedRows`      | `NSOutlineView` / `NSTableView` rows (sidebars, rule lists, file lists)  |

## Core loop

1. `take_ax_snapshot(app_name="…")` → tree of uids like `a42g3` and bboxes
2. Pick the dispatch primitive that matches the target element's role
3. Dispatch: `ax_click` / `ax_set_value` / `ax_select`
4. Re-snapshot to verify the new state

**Invalidation rule:** every fresh `take_ax_snapshot` bumps the generation and invalidates all prior uids. Snapshot immediately before dispatching.

## Example 1 — Press a button

```json
{ "tool": "take_ax_snapshot", "arguments": { "app_name": "TextEdit" } }
```

Find the `AXButton "Save"` with `uid="a12g1"` in the emitted tree.

```json
{ "tool": "ax_click", "arguments": { "uid": "a12g1" } }
```

Expected response:

```json
{ "ok": true, "dispatched_via": "AXPress", "bbox": { "x": 480, "y": 290, "w": 60, "h": 24 } }
```

## Example 2 — Write a text field

```json
{ "tool": "take_ax_snapshot", "arguments": { "app_name": "Finder" } }
```

Find the `AXSearchField` with `uid="a7g2"`.

```json
{ "tool": "ax_set_value", "arguments": { "uid": "a7g2", "text": "invoice.pdf" } }
```

Response:

```json
{ "ok": true, "dispatched_via": "AXSetAttributeValue", "bbox": { "x": 900, "y": 14, "w": 200, "h": 22 } }
```

**Caveat:** `ax_set_value` writes the value attribute directly — it does **not** fire `keydown` / `keyup`, does not participate in IME composition, and does not populate the undo stack. If the app listens for key events (live-search, autocomplete), fall through to the `not_dispatchable` recovery path below.

## Example 3 — Select a sidebar row (System Settings)

Sidebar rows are backed by `NSOutlineView` and typically refuse `AXPress`. Use `ax_select` instead.

```json
{ "tool": "take_ax_snapshot", "arguments": { "app_name": "System Settings" } }
```

Find the `AXRow` with name "Wi-Fi" at `uid="a18g3"` (or any descendant cell of that row — the tool walks up to the enclosing `AXRow`).

```json
{ "tool": "ax_select", "arguments": { "uid": "a18g3" } }
```

Response:

```json
{ "ok": true, "dispatched_via": "AXSelectedRows", "bbox": { "x": 10, "y": 120, "w": 210, "h": 32 } }
```

## Error envelope and fallbacks

Every `ax_*` call returns either `{ "ok": true, ... }` or a structured error:

```json
{
  "error": {
    "code": "not_dispatchable",
    "message": "Element does not support AXPress",
    "fallback": { "x": 500, "y": 300 }
  }
}
```

Common error codes:

| Code               | Meaning                                            | Recovery                                                    |
|--------------------|----------------------------------------------------|-------------------------------------------------------------|
| `snapshot_expired` | The uid is from an older generation                | Call `take_ax_snapshot` again and re-pick the uid           |
| `uid_not_found`    | The uid was never in the current generation        | Snapshot and check the tree                                 |
| `not_dispatchable` | Element doesn't support the requested AX action    | Use `fallback` coordinates with `click` + `type_text`       |
| `no_row_ancestor`  | (`ax_select` only) uid isn't inside an `AXRow`     | Re-snapshot and target the row or a descendant              |
| `ax_error`         | Underlying Accessibility API returned an error     | Check the message; sometimes a re-snapshot + retry works    |

**Coordinate fallback pattern:**

```json
{ "tool": "click", "arguments": { "x": 500, "y": 300 } }
```

Then, if the original intent was text entry:

```json
{ "tool": "type_text", "arguments": { "text": "hello" } }
```

This restores real key events — at the cost of moving the mouse and stealing focus.

## Notes

- The AX tools are **macOS only**. On Windows, use `find_text` (UI Automation) + `click`.
- Apps with restricted AX trees (privacy-focused Electron apps like Signal) may return only top-level containers. Use the [OCR Fallback](./ocr-fallback-and-element-inspection.md) in those cases.
- Because AX dispatch doesn't move the cursor, it's safe to run in the background while the user continues working in another app.
