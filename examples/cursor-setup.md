# Cursor Setup

This example uses the same config path and `npx` command that the built-in `setup` flow detects.

## Quick path

Run:

```bash
npx native-devtools-mcp setup
```

If `~/.cursor/mcp.json` exists, `setup` can detect it and offer to add the MCP config for you.

## Manual config

File:

```text
~/.cursor/mcp.json
```

A minimal config looks like:

```json
{
  "mcpServers": {
    "native-devtools": {
      "command": "npx",
      "args": ["-y", "native-devtools-mcp"]
    }
  }
}
```

## Smoke test prompt

Once configured, try:

```text
Use native-devtools to focus the Calculator app, take a screenshot, and tell me whether the window is visible.
```
