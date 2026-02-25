#!/bin/sh
# ailsd installer script
# Usage: curl -fsSL https://raw.githubusercontent.com/wfh/ailsd/main/install.sh | sh
set -e

REPO="wfh/ailsd"
BINARY="ailsd"
INSTALL_DIR="${AILSD_INSTALL_DIR:-/usr/local/bin}"

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

    if [ -w "$INSTALL_DIR" ]; then
        mv "${tmp}/${BINARY}" "${INSTALL_DIR}/${BINARY}"
    else
        echo "Installing to ${INSTALL_DIR} (requires sudo)..."
        sudo mv "${tmp}/${BINARY}" "${INSTALL_DIR}/${BINARY}"
    fi

    echo ""
    echo "${BINARY} ${version} installed to ${INSTALL_DIR}/${BINARY}"
    echo "Run '${BINARY}' to get started."
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
