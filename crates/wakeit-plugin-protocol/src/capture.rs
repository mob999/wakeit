pub fn preview(text: &str) -> serde_json::Value {
    preview_with_limit(text, 64 * 1024)
}

fn preview_with_limit(text: &str, limit: usize) -> serde_json::Value {
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    serde_json::json!({"truncated":true,"preview":&text[..end]})
}

/// Keep a value only while it fits its presentation/transport budget.
pub fn bounded_value(value: serde_json::Value, limit: usize) -> serde_json::Value {
    bounded_value_with_preview(value, limit, 64 * 1024)
}

pub fn bounded_value_with_preview(
    value: serde_json::Value,
    limit: usize,
    preview_limit: usize,
) -> serde_json::Value {
    if serde_json::to_vec(&value).is_ok_and(|encoded| encoded.len() <= limit) {
        return value;
    }
    preview_with_limit(
        &value
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| value.to_string()),
        preview_limit,
    )
}
