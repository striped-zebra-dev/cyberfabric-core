//! Data Plane implementation for OAGW.
//!
//! This crate implements the Data Plane component responsible for:
//! - Proxy orchestration and request execution
//! - Plugin execution (auth, guard, transform)
//! - HTTP calls to external upstream services

pub mod module;
pub use module::OagwDpModule;

pub(crate) mod plugin;
pub(crate) mod proxy;
pub(crate) mod rate_limit;

#[cfg(any(test, feature = "test-utils"))]
pub mod test_support;
