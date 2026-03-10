# Android Quickstart

Android support is built into every release. No feature flag or separate build is required.

## Prerequisites

- ADB installed on the host machine
- USB debugging enabled on the device
- A connected device visible to `adb devices`

## Basic flow

1. List connected devices
2. Connect to one device with `android_connect`
3. Find text or take a screenshot
4. Tap the target coordinates

## Example

List devices:

```json
{ "tool": "android_list_devices", "arguments": {} }
```

Example result:

```json
[
  { "serial": "abc123", "state": "device" }
]
```

Connect:

```json
{
  "tool": "android_connect",
  "arguments": {
    "serial": "abc123"
  }
}
```

Find a button by text:

```json
{
  "tool": "android_find_text",
  "arguments": {
    "text": "Settings"
  }
}
```

Example result:

```json
[
  {
    "text": "Settings",
    "x": 540,
    "y": 800,
    "bounds": { "x": 480, "y": 760, "width": 120, "height": 80 }
  }
]
```

Tap it:

```json
{
  "tool": "android_click",
  "arguments": {
    "x": 540,
    "y": 800
  }
}
```

Verify with a screenshot:

```json
{ "tool": "android_screenshot", "arguments": {} }
```

## Notes

- Android coordinates are absolute screen pixels
- Prefer `android_find_text` for text elements before using screenshot-based guessing
- To switch devices, call `android_disconnect` first, then `android_connect` to the new device

## MIUI / HyperOS note

Some Xiaomi, Redmi, and POCO devices require the extra developer option:

```text
USB debugging (Security settings)
```

Without it, input tools may fail with `INJECT_EVENTS permission` errors and `android_find_text` may fail with `could not get idle state`
