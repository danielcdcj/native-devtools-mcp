# Claude Desktop Setup

This example follows the current setup behavior documented in the README and detected by the built-in `setup` command.

## Quick path

Run:

```bash
npx native-devtools-mcp setup
```

If the Claude Desktop config file already exists, `setup` can detect it and offer to add the MCP config for you.

Restart Claude Desktop after configuration.

## macOS

On macOS, Claude Desktop should use the signed app bundle binary rather than `npx`.

Config file:

```text
~/Library/Application Support/Claude/claude_desktop_config.json
```

Minimal config:

```json
{
  "mcpServers": {
    "native-devtools": {
      "command": "/Applications/NativeDevtools.app/Contents/MacOS/native-devtools-mcp"
    }
  }
}
```

### macOS note

Gatekeeper blocks the `npx` path for Claude Desktop on macOS. Use the signed app bundle from GitHub Releases, place it in `/Applications`, then run `setup`.

## Windows

Config file:

```text
%APPDATA%\Claude\claude_desktop_config.json
```

Minimal config:

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

Requires Node.js 18+.

## Smoke test prompt

Once configured, try:

```text
Use native-devtools to list open windows, focus one visible app, take a screenshot, and tell me what text is on screen.
```
