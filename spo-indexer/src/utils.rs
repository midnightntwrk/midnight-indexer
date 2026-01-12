pub fn remove_hex_prefix(s: String) -> String {
    if let Some(stripped) = s.strip_prefix("0x") {
        stripped.to_owned()
    } else {
        s
    }
}

pub fn hex_to_bytes(s: &str) -> Vec<u8> {
    let hex = remove_hex_prefix(s.to_owned());
    hex::decode(hex).expect("input should be valid hex string")
}
