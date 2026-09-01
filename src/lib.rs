//! Apple Universal Links diagnostics: fetching, the MCP tool surface, and rendering.
//!
//! All association-file semantics live in [`blazingly_aasa`]. This crate adds the three things
//! that crate deliberately refuses to carry — a network, a protocol, and opinions about how to
//! present an answer — and nothing else.
//!
//! The binary is a thin shell over these modules. They are public so the integration tests can
//! drive the same catalog the binary serves, rather than a copy of it.

pub mod fetch;
pub mod mcp;
pub mod render;
pub mod tools;
