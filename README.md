# lsc

A CLI for chatting with LangSmith deployments.

```
   ▄█▀▀█▄
  ▄██▄░▄█    lsc v0.1.0
  ███████    https://your-deployment.langgraph.app
  ▀█░░░█     ~/.lsc/config.yaml
   █▀ █▀
```

## Installation

### Homebrew (macOS/Linux)

```bash
brew tap wfh/tap
brew install lsc
```

### Cargo

```bash
cargo install --git https://github.com/wfh/lsc.git
```

### From Source

```bash
git clone https://github.com/wfh/lsc.git
cd lsc
make build
./target/release/lsc
```

## Usage

```bash
# Start a new conversation (configure on first run)
lsc

# Resume an existing thread
lsc --resume
```

### Slash Commands

Type `/` to see available commands:

- `/configure` - Update connection settings
- `/quit` - Exit the chat
- `/exit` - Exit the chat

### Keyboard Shortcuts

- `Enter` - Send message
- `Shift+Enter` - New line
- `Ctrl+C` (twice) - Exit
- `Ctrl+D` - Exit immediately
- `Tab` / `↑↓` - Navigate command completions

## Configuration

On first run, you'll be prompted to configure:

1. **Endpoint URL** - Your LangSmith deployment URL
2. **Authentication** - None, API key, or custom headers
3. **Assistant ID** - The graph/assistant to use

Config is stored at `~/.lsc/config.yaml`

## Development

```bash
make build     # Build the binary
make test      # Run tests
make lint      # Run clippy
make format    # Format code
```

## License

MIT
