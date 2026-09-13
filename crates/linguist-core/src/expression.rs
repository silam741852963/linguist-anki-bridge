//! Deterministic expression normalization shared by collection lookup paths.

/// Match the Python lookup policy: ignore simple HTML wrappers and entity
/// spelling, then compare Unicode whitespace-normalized text exactly.
pub fn normalize_expression(value: &str) -> String {
    let without_tags = strip_html_tags(value);
    let decoded = decode_entities(&without_tags);
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn strip_html_tags(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut in_tag = false;
    for character in value.chars() {
        match character {
            '<' if !in_tag => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => output.push(character),
            _ => {}
        }
    }
    output
}

fn decode_entities(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut remaining = value;
    while let Some(start) = remaining.find('&') {
        output.push_str(&remaining[..start]);
        let entity = &remaining[start + 1..];
        let Some(end) = entity.find(';') else {
            output.push_str(&remaining[start..]);
            return output;
        };
        let token = &entity[..end];
        if let Some(character) = decode_entity(token) {
            output.push(character);
        } else {
            output.push('&');
            output.push_str(token);
            output.push(';');
        }
        remaining = &entity[end + 1..];
    }
    output.push_str(remaining);
    output
}

fn decode_entity(token: &str) -> Option<char> {
    match token {
        "amp" => Some('&'),
        "apos" | "#39" => Some('\''),
        "quot" => Some('"'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "nbsp" => Some(' '),
        _ => {
            let number = token
                .strip_prefix("#x")
                .or_else(|| token.strip_prefix("#X"))
                .and_then(|digits| u32::from_str_radix(digits, 16).ok())
                .or_else(|| {
                    token
                        .strip_prefix('#')
                        .and_then(|digits| digits.parse::<u32>().ok())
                })?;
            char::from_u32(number)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_expression;

    #[test]
    fn normalizes_html_entities_and_whitespace() {
        assert_eq!(normalize_expression(" <b>食べる</b>&nbsp; "), "食べる");
        assert_eq!(normalize_expression("a&#x20;b"), "a b");
        assert_eq!(normalize_expression("\n\t"), "");
    }
}
