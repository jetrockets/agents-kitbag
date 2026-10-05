//! claude-toolkit without a window: where Claude Desktop keeps its config,
//! how a token stays out of it, what each integration needs, and whether a
//! configured token still works.
//!
//! Nothing here draws or prompts. External commands go through
//! [`exec::CommandRunner`] and requests through [`http::Http`], so every
//! behaviour is tested against fakes.

pub mod backup;
pub mod claude;
pub mod config;
pub mod exec;
pub mod files;
pub mod gh;
pub mod health;
pub mod http;
pub mod integrations;
pub mod migrate;
pub mod op;
pub mod packages;
pub mod platform;
pub mod prereq;
pub mod runner;
pub mod secret;
pub mod setup;
pub mod storage;
pub mod store;
pub mod validation;

#[cfg(test)]
pub(crate) mod testing;
