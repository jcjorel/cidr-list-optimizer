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
fn lossless_merge_with_preferred_file_present() {
    let input = tmp_file("10.0.0.0/24\n10.0.1.0/24\n");
    let pref = tmp_file("10.0.0.0/26\n");

    cmd()
        .arg(input.path())
        .arg("--preferred-over-coverage-cidrs")
        .arg(pref.path())
        .assert()
        .success()
        .stdout(contains("10.0.0.0/23"));
}

#[test]
fn preferred_biases_merging() {
    let input = tmp_file("10.0.0.0/32\n10.0.0.63/32\n");
    let pref = tmp_file("10.0.0.0/26\n");

    let output = cmd()
        .arg(input.path())
        .args(["--ipv4-target", "1", "--max-over-coverage", "-1"])
        .arg("--preferred-over-coverage-cidrs")
        .arg(pref.path())
        .args(["--format", "json"])
        .assert()
        .success();

    let json: serde_json::Value =
        serde_json::from_slice(&output.get_output().stdout).unwrap();
    // 10.0.0.0/26 = 64 IPs; 2 input IPs already covered → 62 preferred over-coverage
    assert_eq!(json["stats"]["total_ipv4_preferred_over_coverage"], 62);
}

#[test]
fn non_preferred_over_coverage_cap() {
    let input = tmp_file("10.0.0.0/24\n10.0.2.0/24\n");
    let pref = tmp_file("192.168.0.0/16\n");

    cmd()
        .arg(input.path())
        .args(["--ipv4-target", "1", "--max-over-coverage", "-1"])
        .args(["--max-non-preferred-over-coverage", "0"])
        .arg("--preferred-over-coverage-cidrs")
        .arg(pref.path())
        .assert()
        .code(2)
        .stderr(contains("over-coverage cap prevents further merging"));
}

#[test]
fn preferred_file_not_found() {
    let input = tmp_file("10.0.0.0/24\n");

    cmd()
        .arg(input.path())
        .arg("--preferred-over-coverage-cidrs")
        .arg("/nonexistent/path.txt")
        .assert()
        .failure()
        .stderr(contains("cannot open preferred file"));
}

#[test]
fn lossless_aggregation_unaffected_by_preferred() {
    let input = tmp_file("10.0.0.0/25\n10.0.0.128/25\n");
    let pref = tmp_file("10.0.0.0/24\n");

    cmd()
        .arg(input.path())
        .arg("--preferred-over-coverage-cidrs")
        .arg(pref.path())
        .assert()
        .success()
        .stdout(contains("10.0.0.0/24"));
}

#[test]
fn max_non_preferred_without_preferred_errors() {
    let input = tmp_file("10.0.0.0/24\n");

    cmd()
        .arg(input.path())
        .args(["--max-non-preferred-over-coverage", "5"])
        .assert()
        .failure()
        .stderr(contains("--max-non-preferred-over-coverage requires --preferred-over-coverage-cidrs"));
}

#[test]
fn stats_output_format() {
    let input = tmp_file("10.0.0.0/32\n10.0.0.63/32\n");
    let pref = tmp_file("10.0.0.0/26\n");

    cmd()
        .arg(input.path())
        .args(["--ipv4-target", "1", "--max-over-coverage", "-1"])
        .arg("--preferred-over-coverage-cidrs")
        .arg(pref.path())
        .arg("--stats")
        .assert()
        .success()
        .stderr(contains("Preferred:").and(contains("IPs)")))
        .stderr(contains("Non-preferred:").and(contains("IPs)")));
}

#[test]
fn json_output_includes_preferred_stats() {
    let input = tmp_file("10.0.0.0/32\n10.0.0.63/32\n");
    let pref = tmp_file("10.0.0.0/26\n");

    let output = cmd()
        .arg(input.path())
        .args(["--ipv4-target", "1", "--max-over-coverage", "-1"])
        .arg("--preferred-over-coverage-cidrs")
        .arg(pref.path())
        .args(["--format", "json"])
        .assert()
        .success();

    let json: serde_json::Value =
        serde_json::from_slice(&output.get_output().stdout).unwrap();
    assert!(json["stats"]["total_ipv4_preferred_over_coverage"].as_u64().unwrap() > 0);
    // Field is omitted from JSON when zero (skip_serializing_if); serde_json returns null for absent keys.
    assert!(json["stats"]["total_ipv4_non_preferred_over_coverage"].is_null() || json["stats"]["total_ipv4_non_preferred_over_coverage"] == 0);
}

#[test]
fn source_map_includes_preferred_contributions() {
    let input = tmp_file("10.0.0.0/32\n10.0.0.63/32\n");
    let pref = tmp_file("10.0.0.0/26\n");
    let sm_file = NamedTempFile::new().unwrap();

    cmd()
        .arg(input.path())
        .args(["--ipv4-target", "1", "--max-over-coverage", "-1"])
        .arg("--preferred-over-coverage-cidrs")
        .arg(pref.path())
        .arg("--source-map")
        .arg(sm_file.path())
        .assert()
        .success();

    let sm_content = std::fs::read_to_string(sm_file.path()).unwrap();
    let sm: serde_json::Value = serde_json::from_str(&sm_content).unwrap();
    let entries = sm["entries"].as_array().unwrap();
    assert!(!entries.is_empty());
    let contributions = entries[0]["preferred_contributions"].as_array().unwrap();
    assert!(!contributions.is_empty());
    assert!(contributions[0]["prefix"].as_str().unwrap().contains("10.0.0.0/26"));
}

#[test]
fn multiple_preferred_files() {
    let input = tmp_file("10.0.0.0/32\n10.0.0.63/32\n");
    let pref1 = tmp_file("10.0.0.0/27\n");
    let pref2 = tmp_file("10.0.0.32/27\n");

    let output = cmd()
        .arg(input.path())
        .arg("--preferred-over-coverage-cidrs")
        .arg(pref1.path())
        .arg("--preferred-over-coverage-cidrs")
        .arg(pref2.path())
        .args(["--ipv4-target", "1", "--max-over-coverage", "-1"])
        .args(["--format", "json"])
        .assert()
        .success();

    let json: serde_json::Value =
        serde_json::from_slice(&output.get_output().stdout).unwrap();
    assert!(json["stats"]["total_ipv4_preferred_over_coverage"].as_u64().unwrap() > 0);
}

#[test]
fn preferred_biases_merge_choice_with_target_gt_1() {
    // 4 /24s forming two mergeable /22 candidates, each with gap=512
    let input = tmp_file("10.0.0.0/24\n10.0.2.0/24\n10.0.4.0/24\n10.0.6.0/24\n");
    // Preferred covers the first merge's gap entirely
    let pref = tmp_file("10.0.0.0/22\n");

    let output = cmd()
        .arg(input.path())
        .args(["--ipv4-target", "3", "--max-over-coverage", "-1"])
        .arg("--preferred-over-coverage-cidrs")
        .arg(pref.path())
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    // Preferred merge chosen: 10.0.0.0/22
    assert!(stdout.contains("10.0.0.0/22"));
    // Non-preferred pair NOT merged (remain as individual /24s)
    assert!(stdout.contains("10.0.4.0/24"));
    assert!(stdout.contains("10.0.6.0/24"));
}

#[test]
fn preferred_file_changes_merge_order() {
    let input = tmp_file("10.0.0.0/24\n10.0.2.0/24\n10.0.4.0/24\n10.0.6.0/24\n");
    let pref = tmp_file("10.0.0.0/22\n");

    // Run with preferred file
    let output_with = cmd()
        .arg(input.path())
        .args(["--ipv4-target", "3", "--max-over-coverage", "-1"])
        .arg("--preferred-over-coverage-cidrs")
        .arg(pref.path())
        .args(["--format", "json"])
        .assert()
        .success();

    let json_with: serde_json::Value =
        serde_json::from_slice(&output_with.get_output().stdout).unwrap();
    assert!(json_with["stats"]["total_ipv4_preferred_over_coverage"].as_u64().unwrap() > 0);

    // Run without preferred file
    let output_without = cmd()
        .arg(input.path())
        .args(["--ipv4-target", "3", "--max-over-coverage", "-1"])
        .args(["--format", "json"])
        .assert()
        .success();

    let json_without: serde_json::Value =
        serde_json::from_slice(&output_without.get_output().stdout).unwrap();
    // Field absent when no preferred file is used
    assert!(json_without["stats"]["total_ipv4_preferred_over_coverage"].is_null());
}

#[test]
fn ipv6_preferred_zones() {
    let input = tmp_file("2001:db8::0/48\n2001:db8:1::0/48\n");
    let pref = tmp_file("2001:db8::/47\n");

    let output = cmd()
        .arg(input.path())
        .args(["--ipv6-target", "1", "--max-over-coverage", "-1"])
        .arg("--preferred-over-coverage-cidrs")
        .arg(pref.path())
        .args(["--format", "json"])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(stdout.contains("2001:db8::/47"));

    let json: serde_json::Value =
        serde_json::from_slice(&output.get_output().stdout).unwrap();
    // Lossless merge: two sibling /48s → /47, no over-coverage
    assert!(json["stats"]["total_ipv6_preferred_over_coverage"].is_null() || json["stats"]["total_ipv6_preferred_over_coverage"] == 0);
}

#[test]
fn empty_preferred_file_succeeds() {
    let input = tmp_file("10.0.0.0/24\n10.0.1.0/24\n");
    let pref = tmp_file("\n");

    cmd()
        .arg(input.path())
        .arg("--preferred-over-coverage-cidrs")
        .arg(pref.path())
        .assert()
        .success()
        .stdout(contains("10.0.0.0/23"));
}

#[test]
fn invalid_cidr_in_preferred_file_errors() {
    let input = tmp_file("10.0.0.0/24\n");
    let pref = tmp_file("not-a-cidr\n");

    cmd()
        .arg(input.path())
        .arg("--preferred-over-coverage-cidrs")
        .arg(pref.path())
        .assert()
        .failure()
        .stderr(contains("invalid IP or CIDR"));
}
