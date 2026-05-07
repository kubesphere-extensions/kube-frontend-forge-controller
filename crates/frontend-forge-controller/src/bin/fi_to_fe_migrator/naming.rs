use super::*;

pub(crate) fn migrated_fe_name(fi_name: &str) -> String {
    let raw = format!("fi-{fi_name}");
    if raw.len() <= 63 {
        return raw;
    }
    let hash = sha256_hex(fi_name.as_bytes())
        .chars()
        .take(12)
        .collect::<String>();
    let prefix_len = 63 - "fi-".len() - "-".len() - hash.len();
    let slice = dns_label_prefix(fi_name, prefix_len);
    format!("fi-{slice}-{hash}")
}

pub(crate) fn dns_label_prefix(value: &str, max_len: usize) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if out.len() >= max_len {
            break;
        }
        let normalized = if ch.is_ascii_alphanumeric() || ch == '-' {
            ch.to_ascii_lowercase()
        } else {
            '-'
        };
        out.push(normalized);
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "x".to_string()
    } else {
        trimmed
    }
}
