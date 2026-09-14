//! dict.cc result-table parser; callers own locale and transport selection.

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Translation {
    pub source: String,
    pub target: String,
}

pub fn parse_html(html: &str) -> Vec<Translation> {
    let cells = td_texts(html);
    cells
        .chunks_exact(2)
        .filter(|pair| !pair[0].is_empty() && !pair[1].is_empty())
        .map(|pair| Translation {
            source: pair[0].clone(),
            target: pair[1].clone(),
        })
        .collect()
}
fn td_texts(html: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find("<td") {
        rest = &rest[start..];
        let Some(open) = rest.find('>') else { break };
        rest = &rest[open + 1..];
        let Some(end) = rest.find("</td>") else { break };
        values.push(text(&rest[..end]));
        rest = &rest[end + 5..];
    }
    values
}
fn text(value: &str) -> String {
    let mut result = String::new();
    let mut tag = false;
    for ch in value.chars() {
        match ch {
            '<' => tag = true,
            '>' => tag = false,
            _ if !tag => result.push(ch),
            _ => {}
        }
    }
    result
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pairs_result_columns() {
        assert_eq!(
            parse_html(
                r#"<tr><td>Haus</td><td><a>house</a></td></tr><tr><td>Heim</td><td>home</td></tr>"#
            ),
            vec![
                Translation {
                    source: "Haus".into(),
                    target: "house".into()
                },
                Translation {
                    source: "Heim".into(),
                    target: "home".into()
                }
            ]
        );
    }
}
