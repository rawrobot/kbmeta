use crate::pipeline::error::CookError;
use crate::provider::error::{ProviderError, Retry};

/// Process exit codes, documented for wrapper scripts:
///
/// | Code | Meaning                                        |
/// |------|------------------------------------------------|
/// | 0    | Success.                                       |
/// | 1    | General error (usage, bad input, I/O).         |
/// | 2    | Permanent error — retrying identically is     |
/// |      | pointless (missing API key, bad URL, 4xx,     |
/// |      | bad metadata).                                |
/// | 3    | Transient error — retry after a delay         |
/// |      | (timeout, rate limit, 5xx).                   |
pub const EXIT_GENERAL: i32 = 1;
pub const EXIT_PERMANENT: i32 = 2;
pub const EXIT_TRANSIENT: i32 = 3;

/// Maps an application error to an exit code.
///
/// Walks the error chain, classifying known subsystem errors
/// ([`ProviderError`], [`CookError`]). Anything unrecognized falls back to a
/// general error.
pub fn exit_code(err: &anyhow::Error) -> i32 {
    for cause in err.chain() {
        if let Some(pe) = cause.downcast_ref::<ProviderError>() {
            return match pe.retry() {
                Retry::Permanent => EXIT_PERMANENT,
                Retry::Transient => EXIT_TRANSIENT,
            };
        }
        if let Some(ce) = cause.downcast_ref::<CookError>() {
            return match ce.retry() {
                Retry::Permanent => EXIT_PERMANENT,
                Retry::Transient => EXIT_TRANSIENT,
            };
        }
    }
    EXIT_GENERAL
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::error::CookError;
    use crate::provider::error::{ProviderError, Retry};
    use std::path::PathBuf;

    fn wrap<E>(e: E) -> anyhow::Error
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        anyhow::Error::new(e)
            .context("wrapped in more context")
            .context("and another layer")
    }

    #[test]
    fn provider_permanent_is_exit_2() {
        let err = wrap(ProviderError::MissingApiKey("FOO".into()));
        assert_eq!(exit_code(&err), EXIT_PERMANENT);
    }

    #[test]
    fn provider_transient_is_exit_3() {
        let err = wrap(ProviderError::HttpStatus {
            status: 429,
            message: "rate limited".into(),
            transitive: Retry::Transient,
        });
        assert_eq!(exit_code(&err), EXIT_TRANSIENT);
    }

    #[test]
    fn cook_collision_is_exit_2() {
        let err = wrap(CookError::OutputCollision { path: PathBuf::from("a.md") });
        assert_eq!(exit_code(&err), EXIT_PERMANENT);
    }

    #[test]
    fn general_error_is_exit_1() {
        let err: anyhow::Error = anyhow::anyhow!("boom");
        assert_eq!(exit_code(&err), EXIT_GENERAL);
    }
}
