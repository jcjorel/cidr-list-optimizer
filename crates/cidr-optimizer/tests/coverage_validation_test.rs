use cidr_optimizer::{optimize, validate_coverage, OptimizerConfig, TargetSpec};
use ipnet::IpNet;

#[test]
fn lossless_optimization_covers_all_inputs() {
    let prefixes: Vec<IpNet> = vec![
        "10.0.0.0/24".parse().unwrap(),
        "10.0.1.0/24".parse().unwrap(),
        "192.168.0.0/16".parse().unwrap(),
    ];
    let config = OptimizerConfig::default();
    let result = optimize(&prefixes, &config).unwrap();
    assert!(validate_coverage(&prefixes, &result.entries));
}

#[test]
fn lossy_optimization_covers_all_inputs() {
    // Force lossy by setting target=1 with scattered prefixes
    let prefixes: Vec<IpNet> = vec![
        "10.0.0.0/24".parse().unwrap(),
        "10.0.1.0/24".parse().unwrap(),
        "10.0.4.0/24".parse().unwrap(),
        "10.0.5.0/24".parse().unwrap(),
    ];
    let config = OptimizerConfig {
        ipv4_target: Some(TargetSpec::EntryCount(1)),
        ..Default::default()
    };
    let result = optimize(&prefixes, &config).unwrap();
    // Lossy optimization widens prefixes — should still cover all inputs
    assert!(validate_coverage(&prefixes, &result.entries));
}

#[test]
fn optimize_does_not_return_error_on_coverage_loss() {
    // The optimize function itself does NOT validate coverage.
    // It always returns Ok(...) as long as the algorithm runs.
    // Coverage validation is a separate step via validate_coverage().
    let prefixes: Vec<IpNet> = vec![
        "10.0.0.1/32".parse().unwrap(),
        "10.0.0.2/32".parse().unwrap(),
        "192.168.0.1/32".parse().unwrap(),
        "192.168.0.2/32".parse().unwrap(),
    ];
    let config = OptimizerConfig {
        ipv4_target: Some(TargetSpec::EntryCount(1)),
        ..Default::default()
    };
    // This succeeds even though aggressive lossy merging happens
    let result = optimize(&prefixes, &config).unwrap();
    // The output covers inputs because lossy widens (never drops)
    assert!(validate_coverage(&prefixes, &result.entries));
}

#[test]
fn validate_coverage_detects_missing_input() {
    // Manually test validate_coverage with output that doesn't cover an input
    let inputs: Vec<IpNet> = vec![
        "10.0.0.0/24".parse().unwrap(),
        "192.168.1.0/24".parse().unwrap(),
    ];
    // Only optimize a subset — simulate output missing 192.168.1.0/24
    let partial: Vec<IpNet> = vec!["10.0.0.0/24".parse().unwrap()];
    let config = OptimizerConfig::default();
    let result = optimize(&partial, &config).unwrap();
    // validate_coverage should return false
    assert!(!validate_coverage(&inputs, &result.entries));
}
