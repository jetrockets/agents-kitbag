//! MCP setup for Claude Desktop, as a window: every integration, where its
//! token is kept and whether it still works, and a form to set one up.
//!
//! Hide the console on Windows: this is a windowed app. The runner beside it
//! is the console program.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod backend;
mod demo;
mod entrypoint;
mod snapshot;
mod theme;
mod ui;
mod updates;
mod window;
mod worker;

fn main() -> std::process::ExitCode {
    entrypoint::run()
}
