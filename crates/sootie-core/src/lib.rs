pub mod backend;
pub mod browser;
mod config;
pub mod mcp;
pub mod recipe;
mod recipe_learning;
mod recipe_runtime;
pub mod tools;
pub mod types;
mod vision;

pub use backend::{create_backend, DesktopBackend};
pub use mcp::{JsonRpcRequest, JsonRpcResponse, McpServer};
pub use types::{ActionResult, ElementInfo, RuntimeDiagnostic, SootieError, ToolResult};
