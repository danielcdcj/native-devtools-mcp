# Claude Code Setup

This example uses the same config path and `npx` command that the built-in `setup` flow detects.

## Quick path

Run:

```bash
npx native-devtools-mcp setup
```

If `~/.claude.json` exists, `setup` can detect it and offer to add the MCP config for you.

## Manual config

File:

```text
~/.claude.json
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

## Optional permission allowlist

To avoid approving every screenshot or click in Claude Code, add this to:

```text
.claude/settings.local.json
```

```json
{
  "permissions": {
    "allow": ["mcp__native-devtools__*"]
  }
}
```

## Smoke test prompt

Once configured, try a simple prompt such as:

```text
Use native-devtools to list open windows, take a screenshot of the frontmost app, and tell me what text you can see.
```
