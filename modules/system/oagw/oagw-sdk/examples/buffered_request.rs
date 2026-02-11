//! Example: Buffered HTTP request through OAGW
//!
//! This example demonstrates making a simple buffered HTTP request
//! through OAGW to an external service.
//!
//! # Usage
//! ```bash
//! export OAGW_MODE=remote
//! export OAGW_BASE_URL=https://oagw.internal.cf
//! export OAGW_AUTH_TOKEN=your-token-here
//! cargo run --example buffered_request
//! ```

use oagw_sdk::{ErrorSource, Method, OagwClient, OagwClientConfig, Request};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Load configuration from environment
    let config = OagwClientConfig::from_env()?;
    println!("Using OAGW in {:?} mode", if config.is_shared_process() { "SharedProcess" } else { "RemoteProxy" });

    // Create client
    let client = OagwClient::from_config(config)?;

    // Build request
    let request = Request::builder()
        .method(Method::GET)
        .path("/v1/models")
        .build()?;

    println!("Sending request to OpenAI via OAGW...");

    // Execute request
    let response = client.execute("openai", request).await?;

    println!("Response status: {}", response.status());
    println!("Error source: {:?}", response.error_source());

    // Check error source
    if response.error_source() == ErrorSource::Gateway {
        eprintln!("OAGW gateway error - check alias configuration");
    } else if response.error_source() == ErrorSource::Upstream {
        eprintln!("Upstream service error - check API credentials");
    }

    // Parse JSON response
    let data: serde_json::Value = response.json().await?;
    println!("\nResponse data:");
    println!("{}", serde_json::to_string_pretty(&data)?);

    Ok(())
}
