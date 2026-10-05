#!/bin/bash
# Claude MCP Toolkit — Linux Setup

has_command() {
    command -v "$1" &>/dev/null
}

detect_package_manager() {
    if has_command apt-get; then echo "apt"
    elif has_command dnf; then echo "dnf"
    elif has_command pacman; then echo "pacman"
    elif has_command brew; then echo "brew"
    else echo "none"
    fi
}

install_node() {
    local pm
    pm="$(detect_package_manager)"

    echo ""
    echo "  Node.js is not installed. Installing..."
    echo ""

    case "$pm" in
        apt)
            curl -fsSL https://deb.nodesource.com/setup_lts.x | sudo -E bash -
            sudo apt-get install -y nodejs
            ;;
        dnf)
            curl -fsSL https://rpm.nodesource.com/setup_lts.x | sudo bash -
            sudo dnf install -y nodejs
            ;;
        pacman)
            sudo pacman -S --noconfirm nodejs npm
            ;;
        brew)
            brew install node
            ;;
        none)
            echo "  Could not detect package manager (apt, dnf, pacman, brew)."
            echo "  Please install Node.js manually: https://nodejs.org/"
            echo ""
            return 1
            ;;
    esac
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
