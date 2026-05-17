//! Defines the two-tier error hierarchy: [`OptimizeError`] for pure algorithmic
//! failures and [`OptimizerError`] for I/O and parsing failures encountered when
//! reading input from external sources.

use thiserror::Error;

/// Failures that can occur during the core optimization algorithm.
///
/// Returned by [`optimize()`] and [`optimize_with_progress()`]. These errors are
/// independent of input parsing or I/O — see [`OptimizerError`] for the
/// full-stack variant that includes those.
#[derive(Error, Debug)]
pub enum OptimizeError {
    /// The input contained no valid CIDR entries after parsing.
    #[error("no valid entries in input")]
    EmptyInput,

    /// A configuration parameter is invalid (e.g., conflicting targets).
    #[error("invalid config: {message}")]
    InvalidConfig { message: String },

    /// The requested target is below the minimum achievable entry count.
    #[error("target {target} too small (minimum: {minimum})")]
    TargetTooSmall { target: usize, minimum: usize },

    /// Input exceeds the hard safety limit on entry count.
    #[error("input too large: {count} entries exceeds limit of {limit}")]
    InputTooLarge { count: usize, limit: usize },

    /// The prefix trie exceeded u32::MAX nodes — input is pathologically large.
    #[error("trie arena overflow: exceeded u32::MAX nodes")]
    ArenaOverflow,

    /// The caller's progress callback returned `false`, requesting cancellation.
    #[error("optimization cancelled by progress callback")]
    Cancelled,

    /// Internal invariant violation: the output superset does not cover all inputs.
    /// This indicates a bug in the optimizer.
    #[error("coverage invariant violated: output does not cover all input prefixes")]
    CoverageLost,
}

/// End-to-end errors covering input parsing, I/O, and optimization.
///
/// Returned by [`optimize_from_reader()`] and other entry points that accept
/// raw text input. Wraps [`OptimizeError`] for algorithmic failures.
#[derive(Error, Debug)]
pub enum OptimizerError {
    /// An error from the core optimization algorithm (see [`OptimizeError`]).
    #[error(transparent)]
    Optimize(#[from] OptimizeError),

    /// A CIDR entry could not be parsed at the given input line.
    #[error("parse error at line {line}: {message}")]
    Parse { line: usize, message: String },

    /// The target specification string (e.g., "over-coverage=5%") is malformed.
    #[error("invalid target spec: {message}")]
    TargetSpecParse { message: String },

    /// An I/O error occurred while reading input.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify Display output stability for OptimizeError variants.
    #[test]
    fn optimize_error_display() {
        let e = OptimizeError::EmptyInput;
        assert_eq!(e.to_string(), "no valid entries in input");

        let e = OptimizeError::InputTooLarge { count: 20, limit: 10 };
        assert_eq!(e.to_string(), "input too large: 20 entries exceeds limit of 10");

        let e = OptimizeError::TargetTooSmall { target: 0, minimum: 1 };
        assert_eq!(e.to_string(), "target 0 too small (minimum: 1)");
    }

    /// Verify Display output stability for OptimizerError variants.
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
