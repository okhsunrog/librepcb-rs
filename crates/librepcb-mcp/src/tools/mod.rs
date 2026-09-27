//! Tool implementations: synchronous functions on a [`Session`](crate::session::Session)
//! returning a [`ToolOutput`](crate::outcome::ToolOutput), wired to MCP in
//! [`server`](crate::server). Each module documents its tools.

pub mod circuit;
pub mod layout;
pub mod library;
pub mod mutation;
pub mod output;
pub mod project;
