use thiserror::Error;

#[derive(Debug, Error)]
pub enum FlowError {
    #[error("State file not found: {0}")]
    NoState(String),

    #[error("Phase guard: {0}")]
    PhaseGuard(String),

    #[error("Git error: {0}")]
    Git(String),

    #[error("GitHub error: {0}")]
    GitHub(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_state_display() {
        let err = FlowError::NoState("/path/to/state.json".into());
        assert_eq!(err.to_string(), "State file not found: /path/to/state.json");
    }

    #[test]
    fn phase_guard_display() {
        let err = FlowError::PhaseGuard("Plan must be complete".into());
        assert_eq!(err.to_string(), "Phase guard: Plan must be complete");
    }

    #[test]
    fn git_display() {
        let err = FlowError::Git("merge failed".into());
        assert_eq!(err.to_string(), "Git error: merge failed");
    }

    #[test]
    fn github_display() {
        let err = FlowError::GitHub("rate limited".into());
        assert_eq!(err.to_string(), "GitHub error: rate limited");
    }

    #[test]
    fn from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let err: FlowError = io_err.into();
        // Identify variant via Debug format — avoids a `matches!` assertion
        // whose mismatch arm would be uncovered when the expected variant
        // matches (as it always does here).
        assert!(format!("{:?}", err).starts_with("Io("));
        assert!(err.to_string().contains("file missing"));
    }

    #[test]
    fn from_json_error() {
        let json_err = serde_json::from_str::<serde_json::Value>("invalid").unwrap_err();
        let err: FlowError = json_err.into();
        assert!(format!("{:?}", err).starts_with("Json("));
    }

    #[test]
    fn debug_format_contains_variant_name() {
        let err = FlowError::NoState("test".into());
        let debug_str = format!("{:?}", err);
        assert!(debug_str.contains("NoState"));
    }

    #[test]
    fn source_chain_for_io_error() {
        use std::error::Error;
        let io_err = std::io::Error::other("underlying");
        let err: FlowError = io_err.into();
        // Source chain should lead back to the original io::Error.
        assert!(err.source().is_some());
    }

    #[test]
    fn source_chain_for_non_wrapping_variants() {
        use std::error::Error;
        // NoState, PhaseGuard, Git, GitHub don't wrap another error → no source.
        assert!(FlowError::NoState("x".into()).source().is_none());
        assert!(FlowError::PhaseGuard("x".into()).source().is_none());
        assert!(FlowError::Git("x".into()).source().is_none());
        assert!(FlowError::GitHub("x".into()).source().is_none());
    }
}
