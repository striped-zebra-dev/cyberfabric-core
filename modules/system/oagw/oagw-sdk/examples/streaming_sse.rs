//! Example: Streaming SSE response through OAGW
//!
//! This example demonstrates streaming Server-Sent Events (SSE) from
//! OpenAI's chat completions API through OAGW.
//!
//! # Usage
//! ```bash
//! export OAGW_MODE=remote
//! export OAGW_BASE_URL=https://oagw.internal.cf
//! export OAGW_AUTH_TOKEN=your-token-here
//! cargo run --example streaming_sse
//! ```

use oagw_sdk::{ErrorSource, Method, OagwClient, OagwClientConfig, Request};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Load configuration from environment
    let config = OagwClientConfig::from_env()?;
    println!("Using OAGW in {:?} mode\n", if config.is_shared_process() { "SharedProcess" } else { "RemoteProxy" });

    // Create client
    let client = OagwClient::from_config(config)?;

    // Build streaming request
    let request = Request::builder()
        .method(Method::POST)
        .path("/v1/chat/completions")
        .json(&json!({
            "model": "gpt-4",
            "messages": [
                {
                    "role": "user",
                    "content": "Write a haiku about Rust programming"
                }
            ],
            "stream": true,
            "max_tokens": 100
        }))?
        .build()?;

    println!("Sending streaming request to OpenAI via OAGW...\n");

    // Execute request
    let response = client.execute("openai", request).await?;

    println!("Response status: {}", response.status());

    // Check for errors
    if !response.is_success() {
        if response.error_source() == ErrorSource::Gateway {
            eprintln!("OAGW gateway error");
        } else if response.error_source() == ErrorSource::Upstream {
            eprintln!("OpenAI API error");
        }
        return Err("Request failed".into());
    }

    // Convert to SSE stream
    let mut sse_stream = response.into_sse_stream();

    println!("Streaming response:");
    println!("---");

    // Process SSE events
    while let Some(event) = sse_stream.next_event().await? {
        // Check for stream end
        if event.data.contains("[DONE]") {
            break;
        }

        // Parse event data
        match serde_json::from_str::<serde_json::Value>(&event.data) {
            Ok(data) => {
                // Extract content delta
                if let Some(content) = data["choices"][0]["delta"]["content"].as_str() {
                    print!("{}", content);
                    std::io::Write::flush(&mut std::io::stdout())?;
                }
            }
            Err(e) => {
                eprintln!("\nFailed to parse SSE event: {}", e);
            }
        }
    }

    println!("\n---");
    println!("Stream complete!");

    Ok(())
}
