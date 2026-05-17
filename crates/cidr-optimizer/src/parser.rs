use std::io::BufRead;

use ipnet::{IpNet, Ipv4Net, Ipv6Net};

use crate::error::{OptimizeError, OptimizerError};
use crate::types::{ExclusionEntry, InputEntry, ParsedCidr, PreferredEntry};

/// Parsed and partitioned input.
#[derive(Debug)]
pub struct ParsedInput {
    pub ipv4: Vec<(usize, Ipv4Net)>,
    pub ipv6: Vec<(usize, Ipv6Net)>,
    pub input_metadata: Vec<InputEntry>,
    pub total_entries: usize,
    pub parse_warnings: Vec<(usize, String)>,
}

/// Maximum bytes per line to prevent OOM from single-line attacks.
const MAX_LINE_BYTES: usize = 4096;

/// Parse a CIDR list from a reader into structured entries.
///
/// Handles blank lines, `#` full-line comments, inline `# comment` annotations,
/// bare IPs (promoted to /32 or /128), and non-canonical CIDRs (silently truncated).
pub fn parse_cidrs(input: impl BufRead) -> Result<Vec<ParsedCidr>, OptimizerError> {
    let mut results = Vec::new();
    let mut line_buf = String::new();
    let mut line_num: usize = 0;
    let mut reader = input;

    loop {
        line_buf.clear();
        let bytes_read = reader.read_line(&mut line_buf)?;
        if bytes_read == 0 {
            break;
        }
        line_num += 1;

        if line_buf.len() > MAX_LINE_BYTES {
            return Err(OptimizerError::Parse {
                line: line_num,
                message: format!("line exceeds {} byte limit", MAX_LINE_BYTES),
            });
        }

        let trimmed = line_buf.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Split on first '#' to separate CIDR from inline comment
        let (cidr_part, comment) = if let Some(hash_pos) = trimmed.find('#') {
            let cidr = trimmed[..hash_pos].trim();
            let raw_comment = trimmed[hash_pos + 1..].trim();
            let comment = if raw_comment.is_empty() { None } else { Some(raw_comment.to_string()) };
            (cidr, comment)
        } else {
            (trimmed, None)
        };

        let parsed: Result<IpNet, String> = if cidr_part.contains('/') {
            cidr_part.parse::<IpNet>().map_err(|e| e.to_string())
        } else {
            use std::net::IpAddr;
            match cidr_part.parse::<IpAddr>() {
                Ok(IpAddr::V4(ip)) => Ok(IpNet::V4(Ipv4Net::new(ip, 32).unwrap())),
                Ok(IpAddr::V6(ip)) => Ok(IpNet::V6(Ipv6Net::new(ip, 128).unwrap())),
                Err(e) => Err(e.to_string()),
            }
        };

        match parsed {
            Ok(net) => {
                let raw_text = cidr_part.to_string();
                // Normalize non-canonical prefixes
                let prefix = match net {
                    IpNet::V4(v4) => IpNet::V4(v4.trunc()),
                    IpNet::V6(v6) => IpNet::V6(v6.trunc()),
                };
                results.push(ParsedCidr { prefix, raw_text, comment, line_number: line_num });
            }
            Err(_) => {
                return Err(OptimizerError::Parse {
                    line: line_num,
                    message: format!("invalid IP or CIDR: '{}'", &cidr_part[..cidr_part.len().min(100)]),
                });
            }
        }
    }

    Ok(results)
}

/// Parse a CIDR list into exclusion entries, tagging each with the given source name.
pub fn parse_exclusions(input: impl BufRead, source: &str) -> Result<Vec<ExclusionEntry>, OptimizerError> {
    let cidrs = parse_cidrs(input)?;
    Ok(cidrs.into_iter().map(|c| ExclusionEntry {
        prefix: c.prefix,
        source: source.to_owned(),
        comment: c.comment,
    }).collect())
}

/// Parse a CIDR list into preferred entries, tagging each with the given source name.
pub fn parse_preferred(input: impl BufRead, source: &str) -> Result<Vec<PreferredEntry>, OptimizerError> {
    let cidrs = parse_cidrs(input)?;
    Ok(cidrs.into_iter().map(|c| PreferredEntry {
        prefix: c.prefix,
        source: source.to_owned(),
        comment: c.comment,
    }).collect())
}

/// Parse input lines into partitioned IPv4/IPv6 prefix vectors with indices.
///
/// Wraps `parse_cidrs` with max-entries enforcement, metadata storage, and
/// non-canonical warnings — preserving all existing behavior.
pub fn parse_input(
    input: impl BufRead,
    store_metadata: bool,
    max_entries: usize,
) -> Result<ParsedInput, OptimizerError> {
    let cidrs = parse_cidrs(input)?;

    if cidrs.len() > max_entries {
        return Err(OptimizeError::InputTooLarge {
            count: cidrs.len(),
            limit: max_entries,
        }.into());
    }

    let mut ipv4 = Vec::new();
    let mut ipv6 = Vec::new();
    let mut input_metadata = Vec::new();
    let mut warnings = Vec::new();

    for (entry_index, cidr) in cidrs.iter().enumerate() {
        if store_metadata {
            input_metadata.push(InputEntry {
                original: cidr.raw_text.clone(),
                comment: cidr.comment.clone(),
            });
        }

        match cidr.prefix {
            IpNet::V4(v4) => {
                if cidr.raw_text.contains('/') {
                    if let Ok(original) = cidr.raw_text.parse::<Ipv4Net>() {
                        if original != v4 && warnings.len() < 1000 {
                            warnings.push((
                                cidr.line_number,
                                format!("non-canonical CIDR '{}' normalized to '{}'", original, v4),
                            ));
                        }
                    }
                }
                ipv4.push((entry_index, v4));
            }
            IpNet::V6(v6) => {
                if cidr.raw_text.contains('/') {
                    if let Ok(original) = cidr.raw_text.parse::<Ipv6Net>() {
                        if original != v6 && warnings.len() < 1000 {
                            warnings.push((
                                cidr.line_number,
                                format!("non-canonical CIDR '{}' normalized to '{}'", original, v6),
                            ));
                        }
                    }
                }
                ipv6.push((entry_index, v6));
            }
        }
    }

    Ok(ParsedInput {
        ipv4,
        ipv6,
        input_metadata,
        total_entries: cidrs.len(),
        parse_warnings: warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn parse_valid_entries() {
        let input = "10.0.0.0/8\n192.168.1.1\n2001:db8::/32\n";
        let result = parse_input(Cursor::new(input), false, 100).unwrap();
        assert_eq!(result.ipv4.len(), 2);
        assert_eq!(result.ipv6.len(), 1);
        assert_eq!(result.total_entries, 3);
    }

    #[test]
    fn parse_skips_comments_and_blanks() {
        let input = "# comment\n\n10.0.0.0/8\n  # another\n\n";
        let result = parse_input(Cursor::new(input), false, 100).unwrap();
        assert_eq!(result.total_entries, 1);
    }

    #[test]
    fn parse_non_canonical_truncation() {
        let input = "10.0.0.5/24\n";
        let result = parse_input(Cursor::new(input), false, 100).unwrap();
        assert_eq!(result.ipv4.len(), 1);
        assert_eq!(result.ipv4[0].1, "10.0.0.0/24".parse::<Ipv4Net>().unwrap());
        assert_eq!(result.parse_warnings.len(), 1);
    }

    #[test]
    fn parse_max_entries_error() {
        let input = "10.0.0.1\n10.0.0.2\n10.0.0.3\n";
        let result = parse_input(Cursor::new(input), false, 2);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("input too large"));
    }

    #[test]
    fn parse_invalid_line_error() {
        let input = "10.0.0.1\nnot_an_ip\n";
        let result = parse_input(Cursor::new(input), false, 100);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("parse error at line 2"));
    }

    #[test]
    fn parse_stores_strings_when_enabled() {
        let input = "10.0.0.0/8\n192.168.1.1\n";
        let result = parse_input(Cursor::new(input), true, 100).unwrap();
        assert_eq!(result.input_metadata.len(), 2);
        assert_eq!(result.input_metadata[0].original, "10.0.0.0/8");
        assert_eq!(result.input_metadata[0].comment, None);
        assert_eq!(result.input_metadata[1].original, "192.168.1.1");
        assert_eq!(result.input_metadata[1].comment, None);
    }

    #[test]
    fn parse_inline_comment() {
        let input = "10.0.0.0/8 # Office\n";
        let result = parse_input(Cursor::new(input), true, 100).unwrap();
        assert_eq!(result.input_metadata.len(), 1);
        assert_eq!(result.input_metadata[0].original, "10.0.0.0/8");
        assert_eq!(result.input_metadata[0].comment, Some("Office".to_string()));
    }

    #[test]
    fn parse_inline_comment_empty_after_trim() {
        let input = "10.0.0.0/8 #\n";
        let result = parse_input(Cursor::new(input), true, 100).unwrap();
        assert_eq!(result.input_metadata[0].comment, None);
    }

    #[test]
    fn parse_inline_comment_multiple_hashes() {
        let input = "10.0.0.0/8 # comment # more\n";
        let result = parse_input(Cursor::new(input), true, 100).unwrap();
        assert_eq!(result.input_metadata[0].comment, Some("comment # more".to_string()));
    }

    // --- Tests for parse_cidrs ---

    #[test]
    fn parse_cidrs_empty_input() {
        let result = parse_cidrs(Cursor::new("")).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn parse_cidrs_only_comments_and_blanks() {
        let result = parse_cidrs(Cursor::new("# comment\n\n  # another\n")).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn parse_cidrs_valid_entries() {
        let input = "10.0.0.0/24\n192.168.1.1\n2001:db8::/32\n";
        let result = parse_cidrs(Cursor::new(input)).unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].prefix, "10.0.0.0/24".parse::<IpNet>().unwrap());
        assert_eq!(result[0].line_number, 1);
        assert_eq!(result[1].prefix, "192.168.1.1/32".parse::<IpNet>().unwrap());
        assert_eq!(result[1].line_number, 2);
        assert_eq!(result[2].prefix, "2001:db8::/32".parse::<IpNet>().unwrap());
        assert_eq!(result[2].line_number, 3);
    }

    #[test]
    fn parse_cidrs_with_comments() {
        let input = "10.0.0.0/24 # Office network\n192.168.1.0/24\n";
        let result = parse_cidrs(Cursor::new(input)).unwrap();
        assert_eq!(result[0].comment, Some("Office network".to_string()));
        assert_eq!(result[1].comment, None);
    }

    #[test]
    fn parse_cidrs_non_canonical_truncated() {
        let input = "10.0.0.5/24\n";
        let result = parse_cidrs(Cursor::new(input)).unwrap();
        assert_eq!(result[0].prefix, "10.0.0.0/24".parse::<IpNet>().unwrap());
    }

    #[test]
    fn parse_cidrs_invalid_entry() {
        let result = parse_cidrs(Cursor::new("not_valid\n"));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("parse error at line 1"));
    }

    #[test]
    fn parse_cidrs_bare_ipv6() {
        let result = parse_cidrs(Cursor::new("::1\n")).unwrap();
        assert_eq!(result[0].prefix, "::1/128".parse::<IpNet>().unwrap());
    }

    // --- Tests for parse_exclusions ---

    #[test]
    fn parse_exclusions_maps_source() {
        let input = "10.0.0.0/24 # internal\n192.168.0.0/16\n";
        let result = parse_exclusions(Cursor::new(input), "blocklist.txt").unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].prefix, "10.0.0.0/24".parse::<IpNet>().unwrap());
        assert_eq!(result[0].source, "blocklist.txt");
        assert_eq!(result[0].comment, Some("internal".to_string()));
        assert_eq!(result[1].source, "blocklist.txt");
        assert_eq!(result[1].comment, None);
    }

    #[test]
    fn parse_exclusions_empty_input() {
        let result = parse_exclusions(Cursor::new(""), "empty.txt").unwrap();
        assert!(result.is_empty());
    }
}
