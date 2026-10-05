#!/usr/bin/env bats

setup() {
    source "$BATS_TEST_DIRNAME/../../start-linux.sh"
}

# --- detect_package_manager ---

@test "detect: apt when apt-get exists" {
    has_command() { [[ "$1" == "apt-get" ]]; }
    export -f has_command
    run detect_package_manager
    [ "$output" = "apt" ]
}

@test "detect: dnf when only dnf exists" {
    has_command() { [[ "$1" == "dnf" ]]; }
    export -f has_command
    run detect_package_manager
    [ "$output" = "dnf" ]
}

@test "detect: pacman when only pacman exists" {
    has_command() { [[ "$1" == "pacman" ]]; }
    export -f has_command
    run detect_package_manager
    [ "$output" = "pacman" ]
}

@test "detect: brew when only brew exists" {
    has_command() { [[ "$1" == "brew" ]]; }
    export -f has_command
    run detect_package_manager
    [ "$output" = "brew" ]
}

@test "detect: none when nothing exists" {
    has_command() { return 1; }
    export -f has_command
    run detect_package_manager
    [ "$output" = "none" ]
}

@test "detect: apt wins over dnf when both exist" {
    has_command() { [[ "$1" == "apt-get" || "$1" == "dnf" ]]; }
    export -f has_command
    run detect_package_manager
    [ "$output" = "apt" ]
}

@test "detect: dnf wins over pacman when both exist" {
    has_command() { [[ "$1" == "dnf" || "$1" == "pacman" ]]; }
    export -f has_command
    run detect_package_manager
    [ "$output" = "dnf" ]
}

# --- install_node ---

@test "install: calls brew for brew, no sudo" {
    BREW_ARGS=""
    SUDO_CALLED=false
    detect_package_manager() { echo "brew"; }
    brew() { BREW_ARGS="$*"; }
    sudo() { SUDO_CALLED=true; }
    export -f detect_package_manager brew sudo
    export BREW_ARGS SUDO_CALLED

    install_node

    [ "$BREW_ARGS" = "install node" ]
    [ "$SUDO_CALLED" = "false" ]
}

@test "install: calls sudo pacman for pacman" {
    SUDO_ARGS=""
    detect_package_manager() { echo "pacman"; }
    sudo() { SUDO_ARGS="$*"; }
    export -f detect_package_manager sudo

    install_node

    [ "$SUDO_ARGS" = "pacman -S --noconfirm nodejs npm" ]
}

@test "install: calls sudo apt-get for apt" {
    APT_CALLED=false
    detect_package_manager() { echo "apt"; }
    curl() { return 0; }
    sudo() {
        if [[ "$1" == "apt-get" ]]; then APT_CALLED=true; fi
    }
    export -f detect_package_manager curl sudo
    export APT_CALLED

    install_node

    [ "$APT_CALLED" = "true" ]
}

@test "install: calls sudo dnf for dnf" {
    DNF_CALLED=false
    detect_package_manager() { echo "dnf"; }
    curl() { return 0; }
    sudo() {
        if [[ "$1" == "dnf" ]]; then DNF_CALLED=true; fi
    }
    export -f detect_package_manager curl sudo
    export DNF_CALLED

    install_node

    [ "$DNF_CALLED" = "true" ]
}

@test "install: returns 1 with message when no package manager" {
    detect_package_manager() { echo "none"; }
    export -f detect_package_manager

    run install_node

    [ "$status" -eq 1 ]
    [[ "$output" == *"Could not detect package manager"* ]]
}

# --- check_node_installed ---

@test "check_node: returns 0 when node exists" {
    has_command() { [[ "$1" == "node" ]]; }
    export -f has_command
    check_node_installed
}

@test "check_node: returns 1 when node missing" {
    has_command() { return 1; }
    export -f has_command
    run check_node_installed
    [ "$status" -eq 1 ]
}
