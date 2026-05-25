pub(in crate::app) fn json_object_blocks(text: &str) -> Vec<&str> {
    let mut blocks = Vec::new();
    let mut start = None;
    let mut depth = 0_i32;

    for (index, character) in text.char_indices() {
        match character {
            '{' => {
                if depth == 0 {
                    start = Some(index);
                }
                depth += 1;
            }
            '}' => {
                depth -= 1;
                if depth == 0
                    && let Some(start) = start.take()
                {
                    blocks.push(&text[start..=index]);
                }
            }
            _ => {}
        }
    }

    blocks
}

pub(in crate::app) fn json_object_after<'a>(text: &'a str, object_key: &str) -> Option<&'a str> {
    let object_start = text.find(&format!("\"{object_key}\""))?;
    Some(&text[object_start..])
}

pub(in crate::app) fn json_string(text: &str, key: &str) -> Option<String> {
    let key_start = text.find(&format!("\"{key}\""))?;
    let after_key = &text[key_start + key.len() + 2..];
    let colon = after_key.find(':')?;
    let rest = after_key[colon + 1..].trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

pub(in crate::app) fn json_object_string(
    text: &str,
    object_key: &str,
    value_key: &str,
) -> Option<String> {
    let object_start = text.find(&format!("\"{object_key}\""))?;
    json_string(&text[object_start..], value_key)
}

pub(in crate::app) fn json_string_array(text: &str, key: &str) -> Option<Vec<String>> {
    let key_start = text.find(&format!("\"{key}\""))?;
    let after_key = &text[key_start + key.len() + 2..];
    let colon = after_key.find(':')?;
    let rest = after_key[colon + 1..].trim_start();
    let rest = rest.strip_prefix('[')?;
    let mut values = Vec::new();
    let mut remaining = rest;

    loop {
        remaining = remaining.trim_start();
        if remaining.starts_with(']') {
            return Some(values);
        }
        remaining = remaining.strip_prefix('"')?;
        let end = remaining.find('"')?;
        values.push(remaining[..end].to_string());
        remaining = remaining[end + 1..].trim_start();
        if remaining.starts_with(',') {
            remaining = &remaining[1..];
        } else if remaining.starts_with(']') {
            return Some(values);
        } else {
            return None;
        }
    }
}

pub(in crate::app) fn json_u32(text: &str, key: &str) -> Option<u32> {
    let key_start = text.find(&format!("\"{key}\""))?;
    let after_key = &text[key_start + key.len() + 2..];
    let colon = after_key.find(':')?;
    let rest = after_key[colon + 1..].trim_start();
    let end = rest
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(rest.len());

    rest[..end].parse().ok()
}
