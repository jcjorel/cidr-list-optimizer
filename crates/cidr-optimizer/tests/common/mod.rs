use std::net::Ipv4Addr;
use std::net::Ipv6Addr;
use std::time::Instant;

use ipnet::IpNet;

/// Wraps a closure, prints elapsed time to stderr, returns the result.
pub fn time_it<F: FnOnce() -> R, R>(label: &str, f: F) -> R {
    let start = Instant::now();
    let result = f();
    let elapsed = start.elapsed();
    eprintln!("{}: {:.3}s", label, elapsed.as_secs_f64());
    result
}

/// Generates N sequential IPv4 /32s starting from base.
pub fn generate_contiguous_v4(base: Ipv4Addr, count: u32) -> Vec<IpNet> {
    let start = u32::from(base);
    (0..count)
        .map(|i| {
            let addr = Ipv4Addr::from(start + i);
            IpNet::V4(ipnet::Ipv4Net::new(addr, 32).unwrap())
        })
        .collect()
}

/// Generates N sequential IPv6 /128s starting from base.
pub fn generate_contiguous_v6(base: Ipv6Addr, count: u64) -> Vec<IpNet> {
    let start = u128::from(base);
    (0..count)
        .map(|i| {
            let addr = Ipv6Addr::from(start + i as u128);
            IpNet::V6(ipnet::Ipv6Net::new(addr, 128).unwrap())
        })
        .collect()
}

/// Computes expected output count for a contiguous range of /32s starting at `start`.
/// Algorithm: binary decomposition — iteratively subtract the largest
/// power-of-2 aligned block that fits within [start, start+count).
pub fn minimal_prefix_decomposition_count(start: u32, count: u32) -> usize {
    if count == 0 {
        return 0;
    }
    let mut remaining = count;
    let mut pos = start;
    let mut blocks = 0usize;
    while remaining > 0 {
        // Largest power-of-2 block aligned at pos that fits in remaining
        let alignment = if pos == 0 { 1u64 << 32 } else { (pos as u64) & (-(pos as i64) as u64) };
        let max_block = alignment.min(remaining as u64);
        // Round down to power of 2
        let block_size = 1u32 << (31 - (max_block as u32).leading_zeros());
        pos += block_size;
        remaining -= block_size;
        blocks += 1;
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decomposition_power_of_two_aligned() {
        // 65536 contiguous /32s starting at 10.0.0.0 (0x0A000000) → one /16
        assert_eq!(minimal_prefix_decomposition_count(0x0A000000, 65536), 1);
    }

    #[test]
    fn test_decomposition_10000() {
        // 10000 contiguous /32s starting at 10.0.0.0
        // Decomposition: 8192 + 1024 + 512 + 256 + 16 = 10000 → 5 blocks
        let count = minimal_prefix_decomposition_count(0x0A000000, 10000);
        assert_eq!(count, 5);
    }

    #[test]
    fn test_decomposition_zero() {
        assert_eq!(minimal_prefix_decomposition_count(0, 0), 0);
    }

    #[test]
    fn test_decomposition_1m_aligned() {
        // 1048576 = 2^20 starting at 10.0.0.0 (0x0A000000, lower 20 bits = 0) → one /12
        assert_eq!(minimal_prefix_decomposition_count(0x0A000000, 1048576), 1);
    }
}
