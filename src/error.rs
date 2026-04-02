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
        assert!(matches!(err, FlowError::Io(_)));
        assert!(err.to_string().contains("file missing"));
    }

    #[test]
    fn from_json_error() {
        let json_err = serde_json::from_str::<serde_json::Value>("invalid").unwrap_err();
        let err: FlowError = json_err.into();
        assert!(matches!(err, FlowError::Json(_)));
    }
}
