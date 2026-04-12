# Installing MCP Servers

MCP (Model Context Protocol) servers extend the rline AI agent with additional tools. An MCP server is an external process that exposes tools over a JSON-RPC 2.0 protocol via stdio. When the agent starts a conversation, rline automatically spawns configured MCP servers, discovers their tools, and makes them available to the AI model alongside the built-in tools.

## Configuration

MCP servers are configured in JSON files using the same format as Claude Desktop. rline loads configuration from two locations:

1. **Global** (application-wide): `~/.config/rline/mcp.json`
2. **Project** (per-project): `.mcp.json` in the project root directory

If both files define a server with the same name, the project-level configuration takes precedence.

### Configuration format

```json
{
  "mcpServers": {
    "server-name": {
      "command": "executable",
      "args": ["arg1", "arg2"],
      "env": {
        "ENV_VAR": "value"
      },
      "trusted": false
    }
  }
}
```

| Field | Required | Description |
|-------|----------|-------------|
| `command` | Yes | The executable to run (e.g. `"npx"`, `"python"`, `"node"`) |
| `args` | No | Command-line arguments passed to the executable |
| `env` | No | Environment variables set for the server process |
| `trusted` | No | Permission level for the server's tools (default: `false`) |

### Permissions: trusted vs untrusted

The `trusted` field controls whether the server's tools require explicit user approval:

- **`"trusted": false`** (default) -- Every tool call from this server requires you to click "Approve" before it executes. Use this for servers that can modify files, execute code, access the network, or perform any action with side effects.

- **`"trusted": true`** -- Tool calls from this server are auto-approved without prompting. Only use this for servers that are read-only and cannot cause harm, such as documentation lookup servers.

## Examples

### Global configuration (`~/.config/rline/mcp.json`)

Servers you want available in every project:

```json
{
  "mcpServers": {
    "context7": {
      "command": "npx",
      "args": ["-y", "@upstash/context7-mcp@latest"],
      "trusted": true
    }
  }
}
```

The context7 server provides documentation lookup -- it only reads public documentation and cannot modify your system, so it is safe to mark as trusted.

### Project configuration (`.mcp.json` in project root)

Servers specific to a particular project:

```json
{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/home/user/project"],
      "trusted": false
    },
    "playwright": {
      "command": "npx",
      "args": ["@playwright/mcp@latest"],
      "trusted": false
    }
  }
}
```

Both of these servers can perform actions with side effects (file writes, browser automation), so they should remain untrusted.

### Server with environment variables

```json
{
  "mcpServers": {
    "postgres": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-postgres"],
      "env": {
        "PGHOST": "localhost",
        "PGPORT": "5432",
        "PGDATABASE": "myapp_dev",
        "PGUSER": "dev",
        "PGPASSWORD": "devpassword"
      },
      "trusted": false
    }
  }
}
```

## How MCP tools appear in the agent

MCP tools are prefixed with `mcp__{server-name}__{tool-name}` to avoid name collisions with built-in tools. For example, a tool called `query-docs` from a server named `context7` appears as `mcp__context7__query-docs`.

The AI model sees all MCP tools in its tool list and can call them just like built-in tools. In the agent panel UI, MCP tool calls appear as collapsible cards showing the tool name and arguments, with Approve/Deny buttons for untrusted servers.

## Server lifecycle

- MCP servers are started when you send a message to the agent (not when the application launches).
- Each server goes through an initialization handshake before its tools become available.
- If a server fails to start or times out during initialization (30 second limit), it is skipped with a warning -- the agent continues with the remaining servers and built-in tools.
- Server processes are automatically killed when the agent conversation ends or when you start a new task.

## Finding MCP servers

MCP servers are available from many sources. Some well-known ones:

| Server | Package | Description |
|--------|---------|-------------|
| Context7 | `@upstash/context7-mcp` | Documentation lookup for libraries and frameworks |
| Filesystem | `@modelcontextprotocol/server-filesystem` | Read/write files in specified directories |
| Playwright | `@playwright/mcp` | Browser automation and testing |
| PostgreSQL | `@modelcontextprotocol/server-postgres` | Query PostgreSQL databases |

For a full list, see the [MCP Servers repository](https://github.com/modelcontextprotocol/servers).

## Troubleshooting

### Server fails to start

Check that the command is installed and available on your PATH. For `npx`-based servers, ensure Node.js is installed:

```bash
# Verify npx is available
npx --version

# Test the server manually
npx -y @upstash/context7-mcp@latest
```

### No MCP tools appear

1. Verify your config file is valid JSON (no trailing commas, correct quoting).
2. Check that the config file is in the right location:
   - Global: `~/.config/rline/mcp.json`
   - Project: `.mcp.json` in the project root (the directory you opened in rline)
3. Look at the terminal output for warnings about MCP server startup failures.

### Server times out during initialization

Some servers take time to download dependencies on first run (especially `npx`-based ones). The initialization timeout is 30 seconds. If a server consistently times out, try running it manually first to cache its dependencies:

```bash
npx -y @upstash/context7-mcp@latest
```

Then press Ctrl+C once it starts, and try again in rline.
