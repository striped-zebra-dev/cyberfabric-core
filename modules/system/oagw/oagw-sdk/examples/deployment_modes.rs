//! Example: Deployment mode comparison
//!
//! This example shows that the same application code works identically
//! in both SharedProcess and RemoteProxy deployment modes.
//!
//! # Usage
//!
//! ## RemoteProxy mode (production)
//! ```bash
//! export OAGW_MODE=remote
//! export OAGW_BASE_URL=https://oagw.internal.cf
//! export OAGW_AUTH_TOKEN=your-token-here
//! cargo run --example deployment_modes
//! ```
//!
//! ## SharedProcess mode (development)
//! Note: SharedProcess mode requires Control Plane dependency injection,
//! which is not yet implemented. This example demonstrates the API.
//! ```bash
//! export OAGW_MODE=shared
//! cargo run --example deployment_modes
//! ```

use oagw_sdk::{Method, OagwClient, OagwClientConfig, Request};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    println!("=== OAGW Deployment Mode Example ===\n");

    // Load configuration from environment
    // The deployment mode is determined by environment variables,
    // but the application code remains identical!
    let config = OagwClientConfig::from_env()?;

    println!("Deployment mode: {}", if config.is_shared_process() {
        "SharedProcess (direct function calls, zero serialization)"
    } else {
        "RemoteProxy (HTTP requests to OAGW service)"
    });

    // Create client - same code for both modes
    let client = OagwClient::from_config(config)?;

    println!("Client created: {:?}\n", client);

    // Execute request - same code for both modes
    let request = Request::builder()
        .method(Method::GET)
        .path("/v1/models")
        .build()?;

    println!("Executing request...");

    match client.execute("openai", request).await {
        Ok(response) => {
            println!("✓ Request successful!");
            println!("  Status: {}", response.status());
            println!("  Error source: {:?}", response.error_source());

            // Parse response
            match response.json::<serde_json::Value>().await {
                Ok(data) => {
                    println!("\n  Response data:");
                    if let Some(models) = data["data"].as_array() {
                        println!("  Found {} models", models.len());
                        for model in models.iter().take(3) {
                            if let Some(id) = model["id"].as_str() {
                                println!("    - {}", id);
                            }
                        }
                        if models.len() > 3 {
                            println!("    ... and {} more", models.len() - 3);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("  Failed to parse response: {}", e);
                }
            }
        }
        Err(e) => {
            eprintln!("✗ Request failed: {}", e);
            if e.is_retryable() {
                eprintln!("  (This error is retryable)");
            }
        }
    }

    println!("\n=== Key Takeaway ===");
    println!("The application code above works identically in both");
    println!("SharedProcess and RemoteProxy modes. The deployment");
    println!("mode is selected via environment variables only!");

    Ok(())
}
