# lsc - LangSmith Chat CLI

A Go CLI tool for chatting with LangSmith Agent Server deployments.

## Project Structure

```
lsc/
├── main.go                       # CLI entry point, flags, configure wizard
├── Makefile                      # Build, test, lint, format commands
├── .goreleaser.yaml              # GoReleaser config for releases
├── .github/workflows/release.yml # GitHub Actions release workflow
├── internal/
│   ├── config/config.go          # Config file management (~/.lsc/config.yaml)
│   ├── api/
│   │   ├── client.go             # HTTP client for Agent Server API
│   │   ├── sse.go                # Server-Sent Events stream parser
│   │   └── types.go              # Request/response types
│   └── ui/
│       ├── model.go              # Bubbletea chat loop, slash commands
│       ├── styles.go             # Lipgloss styles + parrot logo
│       ├── messages.go           # Message formatting helpers
│       └── picker.go             # Thread picker for --resume
```

## Key Commands

```bash
make build     # Build the binary
make test      # Run tests
make lint      # Run golangci-lint
make format    # Format code
```

## Usage

```bash
lsc              # Start new conversation
lsc --resume     # Pick and resume existing thread
lsc --version    # Show version
```

## Slash Commands

- `/configure` - Update connection settings
- `/quit` - Exit the chat
- `/exit` - Exit the chat

## Keyboard Shortcuts

- `Enter` - Send message
- `Shift+Enter` - New line
- `Tab` / `↑↓` - Navigate command completions
- `Ctrl+C` (twice) - Exit with confirmation
- `Ctrl+D` - Exit immediately

## Configuration

Config stored at `~/.lsc/config.yaml`:
- `endpoint` - LangSmith deployment URL
- `api_key` - Optional API key
- `assistant_id` - Graph/assistant name
- `custom_headers` - Optional custom headers

## API

Connects to LangSmith Agent Server:
- `POST /threads` - Create thread
- `POST /threads/{id}/runs/stream` - Stream run with SSE
- `POST /threads/search` - Search threads
- `GET /threads/{id}/state` - Get thread state
- `GET /threads/{id}?select=values` - Get thread with values

Uses `stream_mode: ["messages-tuple"]` for token streaming.

## Releasing

See RELEASING.md for instructions on:
1. Setting up the homebrew-tap repo
2. Configuring GitHub secrets
3. Creating releases with tags

```bash
git tag v0.1.0
git push origin main --tags
```

## Dependencies

- `github.com/charmbracelet/bubbletea` - TUI framework
- `github.com/charmbracelet/bubbles` - TUI components (textarea, spinner)
- `github.com/charmbracelet/huh` - Interactive forms
- `github.com/charmbracelet/lipgloss` - Terminal styling
- `gopkg.in/yaml.v3` - Config parsing
