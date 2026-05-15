use std::io::BufRead;

use ipnet::{Ipv4Net, Ipv6Net};

use crate::error::{OptimizeError, OptimizerError};

/// Parsed and partitioned input.
#[derive(Debug)]
pub struct ParsedInput {
    pub ipv4: Vec<(usize, Ipv4Net)>,
    pub ipv6: Vec<(usize, Ipv6Net)>,
    pub original_strings: Vec<String>,
    pub total_entries: usize,
    pub parse_warnings: Vec<(usize, String)>,
}

/// Parse input lines into partitioned IPv4/IPv6 prefix vectors with indices.
pub fn parse_input(
    input: impl BufRead,
    store_strings: bool,
    max_entries: usize,
) -> Result<ParsedInput, OptimizerError> {
    let mut ipv4 = Vec::new();
    let mut ipv6 = Vec::new();
    let mut original_strings = Vec::new();
    let mut warnings = Vec::new();
    let mut entry_index: usize = 0;

    for (line_num, line_result) in input.lines().enumerate() {
        let line = line_result?;
        let trimmed = line.trim();

        // Skip comments and blank lines
        if trimmed.is_empty() || trimmed.starts_with('#') {
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

        if store_strings {
            original_strings.push(trimmed.to_string());
        }

        // Try parsing as CIDR first, then as bare IP (with /32 or /128 suffix)
        let parsed: Result<ipnet::IpNet, String> = if trimmed.contains('/') {
            trimmed.parse::<ipnet::IpNet>().map_err(|e| e.to_string())
        } else {
            use std::net::IpAddr;
            match trimmed.parse::<IpAddr>() {
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
                        if truncated != v4 {
                            warnings.push((
                                line_num + 1,
                                format!("non-canonical CIDR '{}' normalized to '{}'", v4, truncated),
                            ));
                        }
                        ipv4.push((entry_index, truncated));
                    }
                    ipnet::IpNet::V6(v6) => {
                        let truncated = v6.trunc();
                        if truncated != v6 {
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
                    message: format!("invalid IP or CIDR: '{}'", trimmed),
                });
            }
        }
    }

    Ok(ParsedInput {
        ipv4,
        ipv6,
        original_strings,
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
        assert_eq!(result.original_strings.len(), 2);
        assert_eq!(result.original_strings[0], "10.0.0.0/8");
        assert_eq!(result.original_strings[1], "192.168.1.1");
    }
}
