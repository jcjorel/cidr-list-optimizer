use thiserror::Error;

/// Errors from optimize() / optimize_iter() — no Parse variant.
#[derive(Error, Debug)]
pub enum OptimizeError {
    #[error("no valid entries in input")]
    EmptyInput,

    #[error("invalid config: {message}")]
    InvalidConfig { message: String },

    #[error("target {target} too small (minimum: {minimum})")]
    TargetTooSmall { target: usize, minimum: usize },

    #[error("input too large: {count} entries exceeds limit of {limit}")]
    InputTooLarge { count: usize, limit: usize },

    #[error("trie arena overflow: exceeded u32::MAX nodes")]
    ArenaOverflow,

    #[error("optimization cancelled by progress callback")]
    Cancelled,

    #[error("coverage invariant violated: output does not cover all input prefixes")]
    CoverageLost,
}

/// Errors from optimize_from_reader() — includes parsing.
#[derive(Error, Debug)]
pub enum OptimizerError {
    #[error(transparent)]
    Optimize(#[from] OptimizeError),

    #[error("parse error at line {line}: {message}")]
    Parse { line: usize, message: String },

    #[error("invalid target spec: {message}")]
    TargetSpecParse { message: String },

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optimize_error_display() {
        let e = OptimizeError::EmptyInput;
        assert_eq!(e.to_string(), "no valid entries in input");

        let e = OptimizeError::InputTooLarge { count: 20, limit: 10 };
        assert_eq!(e.to_string(), "input too large: 20 entries exceeds limit of 10");

        let e = OptimizeError::TargetTooSmall { target: 0, minimum: 1 };
        assert_eq!(e.to_string(), "target 0 too small (minimum: 1)");
    }

    #[test]
    fn optimizer_error_display() {
        let e = OptimizerError::Parse { line: 5, message: "invalid IP".into() };
        assert_eq!(e.to_string(), "parse error at line 5: invalid IP");

        let e = OptimizerError::TargetSpecParse { message: "missing %".into() };
        assert_eq!(e.to_string(), "invalid target spec: missing %");

        let e = OptimizerError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "gone"));
        assert_eq!(e.to_string(), "I/O error: gone");
    }
}
