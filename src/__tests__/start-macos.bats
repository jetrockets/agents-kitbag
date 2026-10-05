#!/usr/bin/env bats

setup() {
    source "$BATS_TEST_DIRNAME/../../Start macOS.command"
}

# --- install_node ---

@test "install_node: calls brew install node when brew exists" {
    BREW_ARGS=""
    has_command() { [[ "$1" == "brew" ]]; }
    brew() { BREW_ARGS="$*"; }
    export -f has_command brew
    export BREW_ARGS

    install_node

    [ "$BREW_ARGS" = "install node" ]
}

@test "install_node: installs homebrew when brew missing" {
    HOMEBREW_INSTALLED=false
    has_command() { return 1; }
    install_homebrew() { HOMEBREW_INSTALLED=true; }
    brew() { return 0; }
    export -f has_command install_homebrew brew
    export HOMEBREW_INSTALLED

    install_node

    [ "$HOMEBREW_INSTALLED" = "true" ]
}

@test "install_node: does not use sudo" {
    SUDO_CALLED=false
    has_command() { [[ "$1" == "brew" ]]; }
    brew() { return 0; }
    sudo() { SUDO_CALLED=true; }
    export -f has_command brew sudo
    export SUDO_CALLED

    install_node

    [ "$SUDO_CALLED" = "false" ]
}

# --- install_homebrew ---

@test "install_homebrew: calls curl for homebrew install script" {
    CURL_ARGS=""
    curl() { CURL_ARGS="$*"; echo ""; }
    /bin/bash() { return 0; }
    eval() { return 0; }
    export -f curl

    run install_homebrew

    [[ "$output" == *"Homebrew not found"* ]]
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

# --- consistency with linux ---

@test "consistency: same entry point as linux (node src/index.js)" {
    grep -q 'node src/index.js' "$BATS_TEST_DIRNAME/../../Start macOS.command"
    grep -q 'node src/index.js' "$BATS_TEST_DIRNAME/../../start-linux.sh"
}

@test "consistency: same npm install flags as linux" {
    grep -q 'npm install --silent' "$BATS_TEST_DIRNAME/../../Start macOS.command"
    grep -q 'npm install --silent' "$BATS_TEST_DIRNAME/../../start-linux.sh"
}
