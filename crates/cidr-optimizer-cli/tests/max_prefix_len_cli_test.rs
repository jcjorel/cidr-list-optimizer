use std::io::Write;

use assert_cmd::Command;
use predicates::str::contains;
use tempfile::NamedTempFile;

fn tmp_file(content: &str) -> NamedTempFile {
    let mut f = NamedTempFile::new().unwrap();
    f.write_all(content.as_bytes()).unwrap();
    f.flush().unwrap();
    f
}

fn cmd() -> Command {
    Command::cargo_bin("cidr-optimizer").unwrap()
}

// --- IPv4 tests ---

#[test]
fn max_prefix_len_v4_truncates_host_routes_to_slash24() {
    // Two /32s in the same /24 — with max_prefix_len_v4=24, both truncate to the
    // same /24 and deduplicate into a single output entry.
    let input = tmp_file("10.0.1.5/32\n10.0.1.200/32\n");

    let output = cmd()
        .arg(input.path())
        .args(["--max-prefix-len-v4", "24"])
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines, vec!["10.0.1.0/24"]);
}

#[test]
fn max_prefix_len_v4_enables_further_aggregation() {
    // Four /32s spanning two adjacent /24s. Without truncation, lossless produces 4
    // entries. With max_prefix_len_v4=24, they become two /24s which then merge to /23.
    let input = tmp_file("10.0.0.1/32\n10.0.0.2/32\n10.0.1.1/32\n10.0.1.2/32\n");

    let output = cmd()
        .arg(input.path())
        .args(["--max-prefix-len-v4", "24"])
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines, vec!["10.0.0.0/23"]);
}

#[test]
fn max_prefix_len_v4_default_preserves_host_routes() {
    // Without --max-prefix-len-v4, /32s remain as-is (default is 32).
    let input = tmp_file("10.0.1.5/32\n10.0.1.200/32\n");

    let output = cmd().arg(input.path()).output().unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines.contains(&"10.0.1.5/32"));
    assert!(lines.contains(&"10.0.1.200/32"));
}

#[test]
fn max_prefix_len_v4_invalid_zero_rejected() {
    let input = tmp_file("10.0.0.0/24\n");

    cmd()
        .arg(input.path())
        .args(["--max-prefix-len-v4", "0"])
        .assert()
        .failure()
        .stderr(contains("max_prefix_len_v4"));
}

// --- IPv6 tests ---

#[test]
fn max_prefix_len_v6_truncates_host_routes_to_slash64() {
    // Two /128s in the same /64 — truncated and deduplicated.
    let input = tmp_file("2001:db8::1/128\n2001:db8::ff/128\n");

    let output = cmd()
        .arg(input.path())
        .args(["--max-prefix-len-v6", "64"])
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines, vec!["2001:db8::/64"]);
}

#[test]
fn max_prefix_len_v6_enables_further_aggregation() {
    // Two /128s in adjacent /64s — truncation to /64 then merge to /63.
    let input = tmp_file("2001:db8::1/128\n2001:db8:0:1::1/128\n");

    let output = cmd()
        .arg(input.path())
        .args(["--max-prefix-len-v6", "64"])
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines, vec!["2001:db8::/63"]);
}

#[test]
fn max_prefix_len_v6_invalid_zero_rejected() {
    let input = tmp_file("2001:db8::/32\n");

    cmd()
        .arg(input.path())
        .args(["--max-prefix-len-v6", "0"])
        .assert()
        .failure()
        .stderr(contains("max_prefix_len_v6"));
}

// --- Mixed: v4 constraint does not affect v6 and vice versa ---

#[test]
fn max_prefix_len_v4_does_not_affect_ipv6() {
    // IPv4 truncated to /24, IPv6 /128 preserved (default v6 limit is 128).
    let input = tmp_file("10.0.0.1/32\n2001:db8::1/128\n");

    let output = cmd()
        .arg(input.path())
        .args(["--max-prefix-len-v4", "24"])
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(lines.contains(&"10.0.0.0/24"));
    assert!(lines.contains(&"2001:db8::1/128"));
    assert_eq!(lines.len(), 2);
}
