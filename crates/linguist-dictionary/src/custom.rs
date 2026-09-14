//! Normalization boundary for user-defined browser extraction schemas.

use serde_json::Value;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CustomEntry {
    pub word: String,
    pub reading: String,
    pub definition: String,
    pub audio_url: Option<String>,
}

pub fn validate_schema(url_template: &str, schema: &Value) -> Result<(), String> {
    if !url_template.contains("{word}") {
        return Err("Dictionary URL template must contain {word}".into());
    }
    let fields = schema
        .get("fields")
        .and_then(Value::as_array)
        .ok_or("Extraction schema requires fields")?;
    if schema
        .get("baseSelector")
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        .is_none()
    {
        return Err("Extraction schema requires a baseSelector".into());
    }
    if fields.is_empty()
        || fields.iter().any(|field| {
            field
                .get("name")
                .and_then(Value::as_str)
                .filter(|v| !v.is_empty())
                .is_none()
                || field
                    .get("selector")
                    .and_then(Value::as_str)
                    .filter(|v| !v.is_empty())
                    .is_none()
        })
    {
        return Err("Each extraction field requires name and selector".into());
    }
    Ok(())
}
pub fn parse_extracted(query: &str, body: &str) -> Result<Option<CustomEntry>, String> {
    let values: Vec<Value> = serde_json::from_str(body).map_err(|error| error.to_string())?;
    let Some(value) = values.first() else {
        return Ok(None);
    };
    let text = |key| match value.get(key) {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join("; "),
        Some(Value::String(value)) => value.clone(),
        Some(value) => value.to_string(),
        None => String::new(),
    };
    Ok(Some(CustomEntry {
        word: {
            let word = text("word");
            if word.is_empty() { query.into() } else { word }
        },
        reading: text("reading"),
        definition: text("definition"),
        audio_url: {
            let audio = text("audio_url");
            (!audio.is_empty()).then_some(audio)
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn validates_and_normalizes_extraction() {
        let schema =
            json!({"baseSelector":".entry","fields":[{"name":"definition","selector":".def"}]});
        assert!(validate_schema("https://x/{word}", &schema).is_ok());
        let entry = parse_extracted(
            "term",
            r#"[{"reading":"r","definition":["one","two"],"audio_url":"https://x/a.mp3"}]"#,
        )
        .unwrap()
        .unwrap();
        assert_eq!(entry.word, "term");
        assert_eq!(entry.definition, "one; two");
    }
}
