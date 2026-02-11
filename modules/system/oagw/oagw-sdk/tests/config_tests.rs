//! Tests for configuration

use oagw_sdk::{ClientMode, OagwClientConfig};
use std::time::Duration;
use temp_env;

#[test]
fn test_remote_proxy_config() {
    let config = OagwClientConfig::remote_proxy(
        "https://oagw.internal.cf".to_string(),
        "test-token".to_string(),
    );

    assert!(config.is_remote_proxy());
    assert!(!config.is_shared_process());

    if let ClientMode::RemoteProxy {
        base_url,
        auth_token,
        ..
    } = config.mode
    {
        assert_eq!(base_url, "https://oagw.internal.cf");
        assert_eq!(auth_token, "test-token");
    } else {
        panic!("Expected RemoteProxy mode");
    }
}

#[test]
fn test_config_with_timeout() {
    let config = OagwClientConfig::remote_proxy(
        "https://oagw.internal.cf".to_string(),
        "test-token".to_string(),
    )
    .with_timeout(Duration::from_secs(60));

    assert_eq!(config.default_timeout, Duration::from_secs(60));

    if let ClientMode::RemoteProxy { timeout, .. } = config.mode {
        assert_eq!(timeout, Duration::from_secs(60));
    } else {
        panic!("Expected RemoteProxy mode");
    }
}

#[test]
fn test_config_from_env_missing_token() {
    // Use temp_env to safely manipulate environment variables
    temp_env::with_vars_unset(
        vec!["OAGW_AUTH_TOKEN", "OAGW_MODE", "OAGW_BASE_URL"],
        || {
            let result = OagwClientConfig::from_env();
            assert!(result.is_err());
            assert!(result.unwrap_err().to_string().contains("OAGW_AUTH_TOKEN"));
        },
    );
}

#[test]
fn test_config_from_env_remote_mode() {
    // Use temp_env to safely set environment variables
    temp_env::with_vars(
        vec![
            ("OAGW_MODE", Some("remote")),
            ("OAGW_BASE_URL", Some("https://test.oagw.cf")),
            ("OAGW_AUTH_TOKEN", Some("test-token-123")),
            ("OAGW_TIMEOUT_SECS", Some("45")),
        ],
        || {
            let config = OagwClientConfig::from_env().unwrap();

            assert!(config.is_remote_proxy());

            if let ClientMode::RemoteProxy {
                base_url,
                auth_token,
                timeout,
            } = config.mode
            {
                assert_eq!(base_url, "https://test.oagw.cf");
                assert_eq!(auth_token, "test-token-123");
                assert_eq!(timeout, Duration::from_secs(45));
            } else {
                panic!("Expected RemoteProxy mode");
            }
        },
    );
}

#[test]
fn test_config_from_env_default_base_url() {
    // Use temp_env to safely set environment variables
    temp_env::with_vars(
        vec![("OAGW_AUTH_TOKEN", Some("test-token"))],
        || {
            temp_env::with_var_unset("OAGW_BASE_URL", || {
                let config = OagwClientConfig::from_env().unwrap();

                if let ClientMode::RemoteProxy { base_url, .. } = config.mode {
                    assert_eq!(base_url, "https://oagw.internal.cf");
                } else {
                    panic!("Expected RemoteProxy mode");
                }
            });
        },
    );
}

#[test]
fn test_config_debug_redacts_token() {
    let config = OagwClientConfig::remote_proxy(
        "https://oagw.internal.cf".to_string(),
        "secret-token-123".to_string(),
    );

    let debug_str = format!("{:?}", config);
    assert!(!debug_str.contains("secret-token-123"));
    assert!(debug_str.contains("[REDACTED]"));
}

