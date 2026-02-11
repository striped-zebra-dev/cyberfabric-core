//! Example: Blocking API for build scripts
//!
//! This example demonstrates using the blocking API to download
//! external resources during the build process.
//!
//! This is suitable for use in build.rs files.
//!
//! # Usage
//! ```bash
//! export OAGW_MODE=remote
//! export OAGW_BASE_URL=https://oagw.internal.cf
//! export OAGW_AUTH_TOKEN=your-token-here
//! cargo run --example blocking_build
//! ```

use oagw_sdk::{Method, OagwClient, OagwClientConfig, Request};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Blocking API example (suitable for build.rs)");

    // Load configuration from environment
    let config = OagwClientConfig::from_env().unwrap_or_else(|e| {
        eprintln!("OAGW not configured: {}", e);
        eprintln!("Falling back to direct download...");
        panic!("OAGW configuration required for this example");
    });

    println!("Using OAGW in {:?} mode", if config.is_shared_process() { "SharedProcess" } else { "RemoteProxy" });

    // Create client
    let client = OagwClient::from_config(config)?;

    // Build request for downloading a JavaScript library from unpkg
    let request = Request::builder()
        .method(Method::GET)
        .path("/elements@9.0.15/web-components.min.js")
        .build()?;

    println!("Downloading web-components.min.js from unpkg via OAGW...");

    // Execute request using blocking API
    let response = client.blocking().execute("unpkg", request)?;

    if !response.is_success() {
        eprintln!("Download failed with status: {}", response.status());
        return Err("Download failed".into());
    }

    // Get response body as bytes
    let bytes = response.bytes_blocking()?;

    println!("Downloaded {} bytes", bytes.len());

    // In a real build.rs, you would write this to a file:
    // std::fs::write("assets/web-components.min.js", bytes)?;

    println!("✓ Download complete!");

    Ok(())
}
