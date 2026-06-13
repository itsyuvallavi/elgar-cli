//! Model Context Protocol foundations.
//!
//! This module defines MCP config and JSON-RPC protocol shapes. Transport
//! execution is intentionally added in later slices.

pub mod client;
pub mod config;
pub mod error;
pub mod http;
pub mod logging;
pub mod protocol;

#[cfg(test)]
mod tests;
