//! OAGW SDK - Client library for Outbound API Gateway
//!
//! This library provides a drop-in replacement HTTP client that routes all requests
//! through OAGW (Outbound API Gateway) for security and observability.
//!
//! # Features
//!
//! - **Deployment-agnostic**: Same code works in SharedProcess and RemoteProxy modes
//! - **Streaming support**: Handle both buffered and streaming responses
//! - **SSE parsing**: Built-in Server-Sent Events support for OpenAI/Anthropic
//! - **Blocking API**: Works in build scripts and non-async contexts
//! - **Error source tracking**: Distinguish gateway vs upstream errors
//!
//! # Examples
//!
//! ## Basic Request
//! ```ignore
//! use oagw_sdk::{OagwClient, OagwClientConfig, Request};
//! use http::Method;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = OagwClientConfig::from_env()?;
//!     let client = OagwClient::from_config(config)?;
//!
//!     let request = Request::builder()
//!         .method(Method::GET)
//!         .path("/v1/models")
//!         .build()?;
//!
//!     let response = client.execute("openai", request).await?;
//!     let data = response.json::<serde_json::Value>().await?;
//!
//!     println!("Models: {:?}", data);
//!     Ok(())
//! }
//! ```
//!
//! ## Streaming SSE
//! ```ignore
//! use oagw_sdk::{OagwClient, Request};
//! use serde_json::json;
//!
//! # async fn example(client: OagwClient) -> Result<(), Box<dyn std::error::Error>> {
//! let request = Request::builder()
//!     .method(http::Method::POST)
//!     .path("/v1/chat/completions")
//!     .json(&json!({
//!         "model": "gpt-4",
//!         "messages": [{"role": "user", "content": "Hello"}],
//!         "stream": true
//!     }))?
//!     .build()?;
//!
//! let response = client.execute("openai", request).await?;
//! let mut sse = response.into_sse_stream();
//!
//! while let Some(event) = sse.next_event().await? {
//!     if event.data.contains("[DONE]") {
//!         break;
//!     }
//!     let data: serde_json::Value = serde_json::from_str(&event.data)?;
//!     if let Some(content) = data["choices"][0]["delta"]["content"].as_str() {
//!         print!("{}", content);
//!     }
//! }
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

// Public modules
pub mod blocking;
pub mod client;
pub mod config;
pub mod error;
pub mod sse;
pub mod types;

// Internal modules (not part of public API)
mod impl_remote;
mod impl_shared;

// Keep existing plugin module (currently a stub)
pub mod plugin;

// Re-export main types for convenience
pub use client::OagwClient;
pub use config::{ClientMode, OagwClientConfig};
pub use error::{ClientError, Result};
pub use sse::{SseEvent, SseEventStream};
pub use types::{Body, ErrorSource, Request, RequestBuilder, Response};

// Re-export http types for convenience
pub use http::{Method, StatusCode};
