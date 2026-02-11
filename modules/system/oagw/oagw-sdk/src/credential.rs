/// The resolved secret material.
#[derive(Debug, Clone)]
pub struct SecretValue {
    value: String,
}

impl SecretValue {
    #[must_use]
    pub fn new(value: String) -> Self {
        Self { value }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// Consume and return the inner value.
    #[must_use]
    pub fn into_inner(self) -> String {
        self.value
    }
}

/// Intentionally does not display the secret value.
impl std::fmt::Display for SecretValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[REDACTED]")
    }
}

/// Errors from credential resolution.
#[derive(Debug, thiserror::Error)]
pub enum CredentialError {
    #[error("credential not found: {0}")]
    NotFound(String),
    #[error("credential error: {0}")]
    Internal(String),
}

/// Trait for resolving secret references to their actual values.
#[async_trait::async_trait]
pub trait CredentialResolver: Send + Sync {
    /// Resolve a secret reference (e.g. `cred://openai-key`) to its value.
    ///
    /// # Errors
    /// Returns `CredentialError::NotFound` if the reference does not exist.
    async fn resolve(&self, secret_ref: &str) -> Result<SecretValue, CredentialError>;
}
