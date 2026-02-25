# ailsd

A CLI for chatting with LangSmith deployments.

```
   ▄█▀▀█▄
  ▄██▄░▄█    ailsd v0.0.1
  ███████    https://your-deployment.langgraph.app
  ▀█░░░█     ~/.ailsd/config.yaml
   █▀ █▀
```

## Installation

### Quick Install (Recommended)

```bash
curl -fsSL https://raw.githubusercontent.com/hinthornw/ailsd/main/install.sh | sh
```

### Homebrew (macOS/Linux)

```bash
brew tap hinthornw/tap
brew install ailsd
```

### Cargo

```bash
cargo install --git https://github.com/hinthornw/ailsd.git
```

### From Source

```bash
git clone https://github.com/hinthornw/ailsd.git
cd ailsd
make build
./target/release/ailsd
```

## Usage

```bash
# Start a new conversation (configure on first run)
ailsd

# Resume an existing thread
ailsd --resume
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

Config is stored at `~/.ailsd/config.yaml`

## Development

```bash
make build     # Build the binary
make test      # Run tests
make lint      # Run clippy
make format    # Format code
```

## License

MIT
