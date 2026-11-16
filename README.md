# joplin-mcp

Fast, standalone Rust [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) server for [Joplin](https://joplinapp.org/)'s local Web Clipper / Data API.

Built as a zero-runtime-dependency single binary to avoid `uvx` / Python cache-locking and startup overhead when multiple agent sessions run concurrently.

## Prerequisites

1. Enable **Web Clipper** in Joplin Desktop (`Tools -> Options -> Web Clipper`).
2. Copy your Joplin Web Clipper authorization token.

## Installation

```sh
cargo install --path .
```

Or build a release binary:

```sh
cargo build --release
cp target/release/joplin-mcp ~/.local/bin/
```

## Configuration

`joplin-mcp` runs over `stdio` by default with zero required arguments.

### Antigravity CLI (`agy`) / MCP JSON

```json
{
  "mcpServers": {
    "joplin": {
      "command": "joplin-mcp",
      "env": {
        "JOPLIN_TOKEN": "your-joplin-web-clipper-token"
      }
    }
  }
}
```

### Environment Variables & CLI Flags

| Environment Variable | CLI Flag | Default | Description |
| :--- | :--- | :--- | :--- |
| `JOPLIN_TOKEN` (or `JOPLIN_API_TOKEN`) | `--token` | — | Joplin Web Clipper API token |
| `JOPLIN_URL` | `--url` | — | Full base URL (overrides `--host` and `--port`) |
| `JOPLIN_HOST` | `--host` | `127.0.0.1` | Joplin Web Clipper host |
| `JOPLIN_PORT` | `--port` | `41184` | Joplin Web Clipper port |
| `JOPLIN_TIMEOUT` | `--timeout` | `30` | HTTP request timeout in seconds |

## Tools

- **Utility**: `ping`
- **Notebooks**: `list_notebooks`, `create_notebook`, `update_notebook`, `delete_notebook`
- **Notes**: `get_note`, `get_note_resources`, `get_links`, `create_note`, `update_note`, `edit_note`, `delete_note`, `find_notes`, `find_in_note`, `find_notes_with_tag`, `find_notes_in_notebook`, `get_all_notes`
- **Tags**: `list_tags`, `create_tag`, `update_tag`, `delete_tag`, `get_tags_by_note`, `tag_note`, `untag_note`
- **Trash**: `restore_from_trash`

## Development

Pre-commit hooks enforce `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, and `cargo test`:

```sh
ln -sf ../../scripts/pre-commit .git/hooks/pre-commit
```
