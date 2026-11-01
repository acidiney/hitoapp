pub fn normalize_isrc(value: &str) -> Option<String> {
    let compact: String = if value.len() == 15 {
        let bytes = value.as_bytes();
        if bytes[2] != b'-' || bytes[6] != b'-' || bytes[9] != b'-' {
            return None;
        }
        value
            .chars()
            .filter(|character| *character != '-')
            .collect()
    } else {
        value.to_owned()
    };
    let bytes = compact.as_bytes();
    let shaped = bytes.len() == 12
        && bytes[..2].iter().all(u8::is_ascii_alphabetic)
        && bytes[2..5].iter().all(u8::is_ascii_alphanumeric)
        && bytes[5..].iter().all(u8::is_ascii_digit);
    shaped.then(|| compact.to_ascii_uppercase())
}
