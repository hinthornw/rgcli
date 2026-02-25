#!/bin/sh
# ailsd installer script
# Usage: curl -fsSL https://raw.githubusercontent.com/hinthornw/ailsd/main/install.sh | sh
set -e

REPO="hinthornw/ailsd"
BINARY="ailsd"
INSTALL_DIR="${AILSD_INSTALL_DIR:-$HOME/.local/bin}"

main() {
    need_cmd curl
    need_cmd tar
    need_cmd uname

    local os arch version url tmp

    os="$(detect_os)"
    arch="$(detect_arch)"
    version="$(get_latest_version)"

    if [ -z "$version" ]; then
        err "could not determine latest version"
    fi

    echo "Installing ${BINARY} ${version} (${os}/${arch})..."

    url="https://github.com/${REPO}/releases/download/${version}/${BINARY}_${version#v}_${os}_${arch}.tar.gz"
    tmp="$(mktemp -d)"
    trap "rm -rf '$tmp'" EXIT

    echo "Downloading ${url}..."
    curl -fsSL "$url" -o "${tmp}/${BINARY}.tar.gz"
    tar -xzf "${tmp}/${BINARY}.tar.gz" -C "$tmp"

    if [ ! -f "${tmp}/${BINARY}" ]; then
        err "binary not found in archive"
    fi

    chmod +x "${tmp}/${BINARY}"

    # Ensure install directory exists
    mkdir -p "$INSTALL_DIR"
    mv "${tmp}/${BINARY}" "${INSTALL_DIR}/${BINARY}"

    echo ""
    echo "${BINARY} ${version} installed to ${INSTALL_DIR}/${BINARY}"

    # Add to PATH if needed
    ensure_path

    echo ""
    echo "Run '${BINARY}' to get started."
}

ensure_path() {
    # Check if INSTALL_DIR is already on PATH
    case ":$PATH:" in
        *":${INSTALL_DIR}:"*) return ;;
    esac

    local export_line="export PATH=\"\$HOME/.local/bin:\$PATH\""
    local rc_file=""

    # Detect shell rc file
    local shell_name
    shell_name="$(basename "${SHELL:-/bin/sh}")"
    case "$shell_name" in
        zsh)  rc_file="$HOME/.zshrc" ;;
        bash)
            if [ -f "$HOME/.bashrc" ]; then
                rc_file="$HOME/.bashrc"
            elif [ -f "$HOME/.bash_profile" ]; then
                rc_file="$HOME/.bash_profile"
            else
                rc_file="$HOME/.bashrc"
            fi
            ;;
        fish)
            # fish uses a different syntax
            local fish_config="$HOME/.config/fish/config.fish"
            mkdir -p "$(dirname "$fish_config")"
            if ! grep -q '.local/bin' "$fish_config" 2>/dev/null; then
                echo "" >> "$fish_config"
                echo "fish_add_path \$HOME/.local/bin" >> "$fish_config"
                echo "Added ~/.local/bin to PATH in ${fish_config}"
                echo "Restart your shell or run: source ${fish_config}"
            fi
            return
            ;;
        *)    rc_file="$HOME/.profile" ;;
    esac

    # Check if already present in rc file
    if [ -f "$rc_file" ] && grep -q '.local/bin' "$rc_file" 2>/dev/null; then
        return
    fi

    echo "" >> "$rc_file"
    echo "# Added by ailsd installer" >> "$rc_file"
    echo "$export_line" >> "$rc_file"
    echo "Added ~/.local/bin to PATH in ${rc_file}"
    echo "Restart your shell or run: source ${rc_file}"
}

detect_os() {
    local os
    os="$(uname -s)"
    case "$os" in
        Linux)  echo "linux" ;;
        Darwin) echo "darwin" ;;
        *)      err "unsupported OS: $os" ;;
    esac
}

detect_arch() {
    local arch
    arch="$(uname -m)"
    case "$arch" in
        x86_64|amd64)   echo "amd64" ;;
        aarch64|arm64)  echo "arm64" ;;
        *)              err "unsupported architecture: $arch" ;;
    esac
}

get_latest_version() {
    curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null \
        | grep '"tag_name"' \
        | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/'
}

need_cmd() {
    if ! command -v "$1" > /dev/null 2>&1; then
        err "need '$1' (command not found)"
    fi
}

err() {
    echo "error: $1" >&2
    exit 1
}

main "$@"
