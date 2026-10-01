pub(in crate::app) fn json_string(text: &str, key: &str) -> Option<String> {
    let key_start = text.find(&format!("\"{key}\""))?;
    let after_key = &text[key_start + key.len() + 2..];
    let colon = after_key.find(':')?;
    let rest = after_key[colon + 1..].trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}
