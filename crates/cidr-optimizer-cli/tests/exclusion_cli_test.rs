use std::io::Write;

use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
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

#[test]
fn exclusion_blocks_merge() {
    let input = tmp_file("10.0.0.0/24\n10.0.2.0/24\n");
    let excl = tmp_file("10.0.1.0/24\n");

    cmd()
        .arg(input.path())
        .args(["--ipv4-target", "1", "--max-over-coverage", "-1"])
        .arg("--exclude-cidr")
        .arg(excl.path())
        .assert()
        .code(2)
        .stderr(contains("exclusion zones prevent"));
}

#[test]
fn exclusion_allows_safe_merge() {
    let input = tmp_file("10.0.4.0/24\n10.0.5.0/24\n");
    let excl = tmp_file("10.0.1.0/24\n");

    cmd()
        .arg(input.path())
        .args(["--ipv4-target", "1", "--max-over-coverage", "-1"])
        .arg("--exclude-cidr")
        .arg(excl.path())
        .assert()
        .success()
        .stdout(contains("10.0.4.0/23"));
}

#[test]
fn exclusion_partial_block() {
    let input = tmp_file("10.0.0.0/24\n10.0.2.0/24\n10.0.4.0/24\n10.0.5.0/24\n");
    let excl = tmp_file("10.0.1.0/24\n");

    let output = cmd()
        .arg(input.path())
        .args(["--ipv4-target", "3", "--max-over-coverage", "-1"])
        .arg("--exclude-cidr")
        .arg(excl.path())
        .assert()
        .success();

    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 3, "expected 3 output lines, got: {:?}", lines);
    assert!(stdout.contains("10.0.4.0/23"));
}

#[test]
fn multiple_exclude_files() {
    let input = tmp_file("10.0.0.0/24\n10.0.1.0/24\n");
    let excl1 = tmp_file("192.168.0.0/16\n");
    let excl2 = tmp_file("172.16.0.0/12\n");

    let output = cmd()
        .arg(input.path())
        .arg("--exclude-cidr")
        .arg(excl1.path())
        .arg("--exclude-cidr")
        .arg(excl2.path())
        .args(["--format", "json"])
        .assert()
        .success();

    let json: serde_json::Value =
        serde_json::from_slice(&output.get_output().stdout).unwrap();
    let sources = json["exclusion_sources"].as_array().unwrap();
    assert_eq!(sources.len(), 2);
    assert_eq!(sources[0]["entry_count"], 1);
    assert_eq!(sources[1]["entry_count"], 1);
}

#[test]
fn json_output_has_exclusion_sources() {
    let input = tmp_file("10.0.0.0/24\n10.0.1.0/24\n");
    let excl = tmp_file("192.168.0.0/16  # some comment\n");

    let output = cmd()
        .arg(input.path())
        .arg("--exclude-cidr")
        .arg(excl.path())
        .args(["--format", "json"])
        .assert()
        .success();

    let json: serde_json::Value =
        serde_json::from_slice(&output.get_output().stdout).unwrap();
    let sources = json["exclusion_sources"].as_array().unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0]["entry_count"], 1);
    // Source field contains the filename
    assert!(sources[0]["source"].as_str().unwrap().len() > 0);
}

#[test]
fn source_map_has_exclusion_collisions() {
    let input = tmp_file("10.0.1.0/24\n");
    let excl = tmp_file("10.0.0.0/16  # big range\n");
    let sm_file = NamedTempFile::new().unwrap();

    cmd()
        .arg(input.path())
        .arg("--exclude-cidr")
        .arg(excl.path())
        .arg("--source-map")
        .arg(sm_file.path())
        .assert()
        .success();

    let sm_content = std::fs::read_to_string(sm_file.path()).unwrap();
    let sm: serde_json::Value = serde_json::from_str(&sm_content).unwrap();
    let entries = sm["entries"].as_array().unwrap();
    assert!(!entries.is_empty());
    // The output prefix 10.0.1.0/24 is inside exclusion 10.0.0.0/16
    let collisions = entries[0]["exclusion_collisions"].as_array().unwrap();
    assert!(!collisions.is_empty());
    assert!(collisions[0]["exclusion_prefix"].as_str().unwrap().contains("10.0.0.0/16"));
}

#[test]
fn warn_on_excluded_input_emits_stderr() {
    let input = tmp_file("10.0.1.0/24\n");
    let excl = tmp_file("10.0.0.0/16\n");

    cmd()
        .arg(input.path())
        .arg("--exclude-cidr")
        .arg(excl.path())
        .arg("--warn-on-excluded-input")
        .assert()
        .success()
        .stderr(contains("warning:").and(contains("overlaps exclusion")));
}

#[test]
fn warn_on_excluded_input_silent_when_no_overlap() {
    let input = tmp_file("192.168.0.0/24\n");
    let excl = tmp_file("10.0.0.0/8\n");

    cmd()
        .arg(input.path())
        .arg("--exclude-cidr")
        .arg(excl.path())
        .arg("--warn-on-excluded-input")
        .assert()
        .success()
        .stderr(predicates::str::is_empty());
}

#[test]
fn exclusion_file_not_found() {
    let input = tmp_file("10.0.0.0/24\n");

    cmd()
        .arg(input.path())
        .arg("--exclude-cidr")
        .arg("/nonexistent/path.txt")
        .assert()
        .failure()
        .stderr(contains("cannot open"));
}

#[test]
fn exclusion_file_with_comments_and_blanks() {
    let input = tmp_file("10.0.0.0/24\n10.0.3.0/24\n");
    let excl = tmp_file(
        "# This is a comment\n\n10.0.1.0/24  # inline comment\n\n# Another comment\n10.0.2.0/24\n",
    );

    let output = cmd()
        .arg(input.path())
        .arg("--exclude-cidr")
        .arg(excl.path())
        .args(["--format", "json"])
        .assert()
        .success();

    let json: serde_json::Value =
        serde_json::from_slice(&output.get_output().stdout).unwrap();
    let sources = json["exclusion_sources"].as_array().unwrap();
    assert_eq!(sources[0]["entry_count"], 2);
}

#[test]
fn exclusion_file_bare_ips() {
    let input = tmp_file("10.0.0.0/24\n10.0.2.0/24\n");
    let excl = tmp_file("10.0.1.1\n");

    cmd()
        .arg(input.path())
        .args(["--ipv4-target", "1", "--max-over-coverage", "-1"])
        .arg("--exclude-cidr")
        .arg(excl.path())
        .assert()
        .code(2);
}

#[test]
fn empty_exclusion_file() {
    let input = tmp_file("10.0.0.0/25\n10.0.0.128/25\n");
    let excl = tmp_file("# only comments\n\n");

    cmd()
        .arg(input.path())
        .arg("--exclude-cidr")
        .arg(excl.path())
        .assert()
        .success()
        .stdout(contains("10.0.0.0/24"));
}

#[test]
fn lossless_mode_ignores_exclusions() {
    let input = tmp_file("10.0.0.0/25\n10.0.0.128/25\n");
    let excl = tmp_file("10.0.0.0/24\n");

    cmd()
        .arg(input.path())
        .arg("--exclude-cidr")
        .arg(excl.path())
        .assert()
        .success()
        .stdout(contains("10.0.0.0/24"));
}

#[test]
fn ipv6_exclusion() {
    let input = tmp_file("2001:db8::/48\n2001:db8:2::/48\n");
    let excl = tmp_file("2001:db8:1::/48\n");

    cmd()
        .arg(input.path())
        .args(["--ipv6-target", "1", "--max-over-coverage", "-1"])
        .arg("--exclude-cidr")
        .arg(excl.path())
        .assert()
        .code(2)
        .stderr(contains("exclusion zones prevent"));
}

#[test]
fn exclusion_constrained_json_stats() {
    let input = tmp_file("10.0.0.0/24\n10.0.2.0/24\n");
    let excl = tmp_file("10.0.1.0/24\n");

    cmd()
        .arg(input.path())
        .args(["--ipv4-target", "1", "--max-over-coverage", "-1"])
        .arg("--exclude-cidr")
        .arg(excl.path())
        .args(["--format", "json"])
        .assert()
        .code(2)
        .stderr(contains("exclusion zones prevent"));
}
