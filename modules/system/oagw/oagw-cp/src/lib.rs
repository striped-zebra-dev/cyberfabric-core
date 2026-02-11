//! Control Plane implementation for OAGW.
//!
//! This crate implements the Control Plane component responsible for:
//! - Configuration management (CRUD for upstreams, routes, plugins)
//! - In-memory repository storage (DashMap-based)
//! - Alias resolution and route matching

pub mod module;
pub use module::OagwCpModule;

pub(crate) mod domain;

#[cfg(any(test, feature = "test-utils"))]
pub mod test_support;
