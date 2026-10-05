#!/bin/bash
# Claude MCP Toolkit — macOS Setup
# Double-click this file to run in Terminal.

has_command() {
    command -v "$1" &>/dev/null
}

install_homebrew() {
    echo "  Homebrew not found. Installing Homebrew first..."
    /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
    eval "$(/opt/homebrew/bin/brew shellenv 2>/dev/null || /usr/local/bin/brew shellenv 2>/dev/null)"
}

install_node() {
    echo ""
    echo "  Node.js is not installed. Installing..."
    echo ""

    if has_command brew; then
        brew install node
    else
        install_homebrew
        brew install node
    fi
}

check_node_installed() {
    has_command node
}

main() {
    cd "$(dirname "$0")"

    if ! check_node_installed; then
        install_node
        if ! check_node_installed; then
            echo ""
            echo "  Failed to install Node.js. Install manually: https://nodejs.org/"
            echo ""
            read -rp "  Press Enter to close..."
            exit 1
        fi
        echo ""
        echo "  Node.js installed successfully."
        echo ""
    fi

    npm install --silent 2>/dev/null
    node src/index.js
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    main "$@"
fi
