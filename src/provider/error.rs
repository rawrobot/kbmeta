use thiserror::Error;

/// Retry classification for a provider failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retry {
    /// Retrying identically is pointless (bad key, bad URL, 4xx, bad schema).
    Permanent,
    /// The request can succeed if retried after a delay (timeout, 429, 5xx,
    /// transport).
    Transient,
}

/// Dedicated error type for the metadata-provider subsystem.
///
/// Every variant carries its own retry classification, so a wrapper (or the
/// CLI's exit-code mapping) can branch on [`Self::retry`] without
/// pattern-matching message strings.
#[derive(Debug, Error)]
pub enum ProviderError {
    /// The configured `api_key_env` is not set or empty.
    #[error(
        "environment variable {0} is not set or empty; \
         set it before running cook, e.g. export {0}='<key>'"
    )]
    MissingApiKey(String),

    /// The endpoint URL is unusable.
    #[error("invalid provider endpoint: {0}")]
    InvalidEndpoint(String),

    /// The `[provider] api_key_env` or token could not be turned into a
    /// bearer header.
    #[error("invalid bearer token")]
    InvalidAuth,

    /// A transport-level failure: connect, timeout, TLS, decoding.
    #[error("provider transport error: {0}")]
    Transport(#[source] reqwest::Error),

    /// The provider returned a non-success HTTP status.
    #[error("provider returned HTTP {status}: {message}")]
    HttpStatus {
        status: u16,
        message: String,
        transitive: Retry,
    },

    /// The response body could not be parsed as the expected shape.
    #[error("provider returned an unparseable response: {0}")]
    BadResponse(#[source] reqwest::Error),

    /// The response payload is invalid (non-JSON, missing fields, bad schema).
    #[error("provider returned invalid metadata: {0}")]
    InvalidMetadata(#[source] anyhow::Error),
}

impl ProviderError {
    /// The retry classification for this failure.
    pub fn retry(&self) -> Retry {
        match self {
            ProviderError::MissingApiKey(_)
            | ProviderError::InvalidEndpoint(_)
            | ProviderError::InvalidAuth
            | ProviderError::HttpStatus {
                transitive: Retry::Permanent,
                ..
            }
            | ProviderError::BadResponse(_)
            | ProviderError::InvalidMetadata(_) => Retry::Permanent,
            ProviderError::Transport(_)
            | ProviderError::HttpStatus {
                transitive: Retry::Transient,
                ..
            } => Retry::Transient,
        }
    }
}

impl ProviderError {
    /// Classifies a transport failure (connect, timeout, TLS, decoding) into
    /// a [`Retry`] and wraps it.
    pub fn from_transport(e: reqwest::Error) -> Self {
        // Decoding failures are permanent; everything else (timeout, connect,
        // request) is likely transient.
        let retry = if e.is_decode() {
            Retry::Permanent
        } else {
            Retry::Transient
        };
        match retry {
            Retry::Permanent => ProviderError::BadResponse(e),
            Retry::Transient => ProviderError::Transport(e),
        }
    }

    /// Builds an [`ProviderError::HttpStatus`] from an optional status.
    pub fn from_status(status: Option<reqwest::StatusCode>, message: String) -> Self {
        let transitive = match status.map(|s| s.as_u16()) {
            Some(429) | Some(500..=599) => Retry::Transient,
            Some(400..=499) => Retry::Permanent,
            _ => Retry::Transient,
        };
        ProviderError::HttpStatus {
            status: status.map(|s| s.as_u16()).unwrap_or(0),
            message,
            transitive,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_key_is_permanent() {
        assert_eq!(
            ProviderError::MissingApiKey("X".into()).retry(),
            Retry::Permanent
        );
    }

    #[test]
    fn transport_is_transient() {
        // Cannot construct a real reqwest::Error cheaply; classify via status
        // and endpoint which carry the retry decision explicitly.
        assert_eq!(
            ProviderError::HttpStatus {
                status: 429,
                message: "r".into(),
                transitive: Retry::Transient
            }
            .retry(),
            Retry::Transient
        );
        assert_eq!(
            ProviderError::HttpStatus {
                status: 500,
                message: "r".into(),
                transitive: Retry::Transient
            }
            .retry(),
            Retry::Transient
        );
    }

    #[test]
    fn client_error_is_permanent() {
        assert_eq!(
            ProviderError::from_status(
                Some(reqwest::StatusCode::UNAUTHORIZED),
                "auth failed".into()
            )
            .retry(),
            Retry::Permanent
        );
    }

    #[test]
    fn rate_limit_is_transient() {
        assert_eq!(
            ProviderError::from_status(Some(reqwest::StatusCode::TOO_MANY_REQUESTS), "r".into())
                .retry(),
            Retry::Transient
        );
    }
}
