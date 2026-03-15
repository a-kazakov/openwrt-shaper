use super::run_cmd;
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

static COUNTER_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"counter packets (\d+) bytes (\d+)").unwrap());

static MARK_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"mark set (0x[0-9a-fA-F]+|\d+)").unwrap());

/// Read upload and download byte counts for a device by mark.
pub fn read_device_bytes(table_name: &str, mark: i32) -> Result<(i64, i64), String> {
    let upload = read_chain_bytes(table_name, "upload", mark)?;
    let download = read_chain_bytes(table_name, "download", mark)?;
    Ok((upload, download))
}

fn read_chain_bytes(table_name: &str, chain: &str, mark: i32) -> Result<i64, String> {
    let output = run_cmd("nft", &["list", "chain", "inet", table_name, chain])?;

    // Match both decimal ("mark set 100") and hex ("mark set 0x00000064")
    let mark_dec = format!("mark set {mark}");
    let mark_hex = format!("mark set 0x{:08x}", mark);
    for line in output.lines() {
        if !line.contains(&mark_dec) && !line.contains(&mark_hex) {
            continue;
        }
        if let Some(caps) = COUNTER_REGEX.captures(line) {
            let bytes: i64 = caps[2]
                .parse()
                .map_err(|e| format!("parse counter bytes: {e}"))?;
            return Ok(bytes);
        }
    }
    Ok(0)
}

pub struct CounterPair {
    pub upload: i64,
    pub download: i64,
}

/// Read byte counters for all devices in both chains.
pub fn read_all_counters(table_name: &str) -> Result<HashMap<i32, CounterPair>, String> {
    let mut result: HashMap<i32, CounterPair> = HashMap::new();

    for chain in ["upload", "download"] {
        let output = run_cmd("nft", &["list", "chain", "inet", table_name, chain])?;

        for line in output.lines() {
            let mark_match = match MARK_REGEX.captures(line) {
                Some(m) => m,
                None => continue,
            };
            let mark: i32 = match parse_mark(&mark_match[1]) {
                Some(m) => m,
                None => continue,
            };

            let counter_match = match COUNTER_REGEX.captures(line) {
                Some(m) => m,
                None => continue,
            };
            let bytes: i64 = match counter_match[2].parse() {
                Ok(b) => b,
                Err(_) => continue,
            };

            let entry = result.entry(mark).or_insert(CounterPair { upload: 0, download: 0 });
            match chain {
                "upload" => entry.upload = bytes,
                _ => entry.download = bytes,
            }
        }
    }

    Ok(result)
}

/// Parse a mark value that may be decimal ("100") or hex ("0x00000064").
fn parse_mark(s: &str) -> Option<i32> {
    if let Some(hex) = s.strip_prefix("0x") {
        i32::from_str_radix(hex, 16).ok()
    } else {
        s.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_counter_regex() {
        let line = "    ip saddr 192.168.1.100 counter packets 1234 bytes 567890 meta mark set 100";
        let caps = COUNTER_REGEX.captures(line).unwrap();
        assert_eq!(&caps[1], "1234");
        assert_eq!(&caps[2], "567890");
    }

    #[test]
    fn parse_mark_regex_decimal() {
        let line = "    ip saddr 192.168.1.100 counter packets 1234 bytes 567890 meta mark set 100";
        let caps = MARK_REGEX.captures(line).unwrap();
        assert_eq!(parse_mark(&caps[1]), Some(100));
    }

    #[test]
    fn parse_mark_regex_hex() {
        let line = "        oifname \"apcli0\" ip saddr 10.255.3.176 counter packets 2693 bytes 944115 meta mark set 0x00000064";
        let caps = MARK_REGEX.captures(line).unwrap();
        assert_eq!(parse_mark(&caps[1]), Some(100));
    }

    #[test]
    fn parse_mark_hex_101() {
        let line = "        oifname \"apcli0\" ip saddr 10.255.3.171 counter packets 52 bytes 11328 meta mark set 0x00000065";
        let caps = MARK_REGEX.captures(line).unwrap();
        assert_eq!(parse_mark(&caps[1]), Some(101));

        let caps = COUNTER_REGEX.captures(line).unwrap();
        assert_eq!(&caps[2], "11328");
    }

    #[test]
    fn no_counter_match() {
        let line = "    chain upload {";
        assert!(COUNTER_REGEX.captures(line).is_none());
    }
}
