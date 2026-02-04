pub fn remove_hex_prefix(s: &str) -> &str {
    s.strip_prefix("0x").unwrap_or(s)
}

pub fn hex_to_bytes(s: &str) -> Vec<u8> {
    let hex = remove_hex_prefix(s);
    hex::decode(hex).expect("input should be valid hex string")
}
