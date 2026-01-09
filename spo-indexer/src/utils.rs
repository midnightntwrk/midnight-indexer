pub fn remove_hex_prefix(s: String) -> String {
    if s.starts_with("0x") {
        s[2..].to_string()
    } else {
        s.to_string()
    }
}

pub fn hex_to_bytes(s: &str) -> Vec<u8> {
    let hex_str = remove_hex_prefix(s.to_string());
    hex::decode(hex_str).expect("input should be valid hex string")
}
