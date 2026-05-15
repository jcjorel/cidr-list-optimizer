use std::io::BufRead;

use ipnet::{Ipv4Net, Ipv6Net};

use crate::error::{OptimizeError, OptimizerError};
use crate::types::InputEntry;

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

/// Maximum number of parse warnings to retain.
const MAX_WARNINGS: usize = 1000;

/// Parse input lines into partitioned IPv4/IPv6 prefix vectors with indices.
pub fn parse_input(
    mut input: impl BufRead,
    store_metadata: bool,
    max_entries: usize,
) -> Result<ParsedInput, OptimizerError> {
    let mut ipv4 = Vec::new();
    let mut ipv6 = Vec::new();
    let mut input_metadata = Vec::new();
    let mut warnings = Vec::new();
    let mut entry_index: usize = 0;
    let mut line_num: usize = 0;
    let mut line_buf = String::new();

    loop {
        line_buf.clear();
        let bytes_read = input.read_line(&mut line_buf)?;
        if bytes_read == 0 {
            break;
        }
        if line_buf.len() > MAX_LINE_BYTES {
            return Err(OptimizerError::Parse {
                line: line_num + 1,
                message: format!("line exceeds {} byte limit", MAX_LINE_BYTES),
            });
        }
        let trimmed = line_buf.trim();

        // Skip comments and blank lines
        if trimmed.is_empty() || trimmed.starts_with('#') {
            line_num += 1;
            continue;
        }

        // Check max entries
        if entry_index >= max_entries {
            return Err(OptimizeError::InputTooLarge {
                count: entry_index + 1,
                limit: max_entries,
            }
            .into());
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

        if store_metadata {
            input_metadata.push(InputEntry {
                original: cidr_part.to_string(),
                comment,
            });
        }

        // Try parsing as CIDR first, then as bare IP (with /32 or /128 suffix)
        let parsed: Result<ipnet::IpNet, String> = if cidr_part.contains('/') {
            cidr_part.parse::<ipnet::IpNet>().map_err(|e| e.to_string())
        } else {
            use std::net::IpAddr;
            match cidr_part.parse::<IpAddr>() {
                Ok(IpAddr::V4(ip)) => Ok(ipnet::IpNet::V4(
                    Ipv4Net::new(ip, 32).unwrap(),
                )),
                Ok(IpAddr::V6(ip)) => Ok(ipnet::IpNet::V6(
                    Ipv6Net::new(ip, 128).unwrap(),
                )),
                Err(e) => Err(e.to_string()),
            }
        };

        match parsed {
            Ok(net) => {
                match net {
                    ipnet::IpNet::V4(v4) => {
                        let truncated = v4.trunc();
                        if truncated != v4 && warnings.len() < MAX_WARNINGS {
                            warnings.push((
                                line_num + 1,
                                format!("non-canonical CIDR '{}' normalized to '{}'", v4, truncated),
                            ));
                        }
                        ipv4.push((entry_index, truncated));
                    }
                    ipnet::IpNet::V6(v6) => {
                        let truncated = v6.trunc();
                        if truncated != v6 && warnings.len() < MAX_WARNINGS {
                            warnings.push((
                                line_num + 1,
                                format!("non-canonical CIDR '{}' normalized to '{}'", v6, truncated),
                            ));
                        }
                        ipv6.push((entry_index, truncated));
                    }
                }
                entry_index += 1;
            }
            Err(_) => {
                return Err(OptimizerError::Parse {
                    line: line_num + 1,
                    message: format!("invalid IP or CIDR: '{}'", &cidr_part[..cidr_part.len().min(100)]),
                });
            }
        }
        line_num += 1;
    }

    Ok(ParsedInput {
        ipv4,
        ipv6,
        input_metadata,
        total_entries: entry_index,
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
    fn parse_non_canonical_warning() {
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
}
