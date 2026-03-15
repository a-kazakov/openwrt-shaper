use super::run_cmd;
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

static COUNTER_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"counter packets (\d+) bytes (\d+)").unwrap());

static MARK_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"mark set (\d+)").unwrap());

/// Read upload and download byte counts for a device by mark.
pub fn read_device_bytes(table_name: &str, mark: i32) -> Result<(i64, i64), String> {
    let upload = read_chain_bytes(table_name, "upload", mark)?;
    let download = read_chain_bytes(table_name, "download", mark)?;
    Ok((upload, download))
}

fn read_chain_bytes(table_name: &str, chain: &str, mark: i32) -> Result<i64, String> {
    let output = run_cmd("nft", &["list", "chain", "inet", table_name, chain])?;

    let mark_str = format!("mark set {mark}");
    for line in output.lines() {
        if !line.contains(&mark_str) {
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

/// Read byte counters for all devices in both chains.
/// Returns map[mark] -> (upload, download).
pub fn read_all_counters(table_name: &str) -> Result<HashMap<i32, [i64; 2]>, String> {
    let mut result: HashMap<i32, [i64; 2]> = HashMap::new();

    for (i, chain) in ["upload", "download"].iter().enumerate() {
        let output = run_cmd("nft", &["list", "chain", "inet", table_name, chain])?;

        for line in output.lines() {
            let mark_match = match MARK_REGEX.captures(line) {
                Some(m) => m,
                None => continue,
            };
            let mark: i32 = match mark_match[1].parse() {
                Ok(m) => m,
                Err(_) => continue,
            };

            let counter_match = match COUNTER_REGEX.captures(line) {
                Some(m) => m,
                None => continue,
            };
            let bytes: i64 = match counter_match[2].parse() {
                Ok(b) => b,
                Err(_) => continue,
            };

            let entry = result.entry(mark).or_insert([0, 0]);
            entry[i] = bytes;
        }
    }

    Ok(result)
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
    fn parse_mark_regex() {
        let line = "    ip saddr 192.168.1.100 counter packets 1234 bytes 567890 meta mark set 100";
        let caps = MARK_REGEX.captures(line).unwrap();
        assert_eq!(&caps[1], "100");
    }

    #[test]
    fn no_counter_match() {
        let line = "    chain upload {";
        assert!(COUNTER_REGEX.captures(line).is_none());
    }
}
