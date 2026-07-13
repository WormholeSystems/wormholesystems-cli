use std::collections::BTreeMap;

/// Replaces the values of matching `KEY=...` lines and keeps everything
/// else verbatim, so the output stays in sync with the upstream template.
/// Keys that never occur in the template are appended at the end.
pub fn patch(template: &str, values: &BTreeMap<String, String>) -> String {
    let mut remaining = values.clone();
    let mut out = String::new();

    for line in template.lines() {
        let patched = line_key(line)
            .and_then(|key| remaining.remove(key).map(|v| (key, v)))
            .map(|(key, value)| format!("{key}={}", quote(&value)));
        out.push_str(patched.as_deref().unwrap_or(line));
        out.push('\n');
    }

    if !remaining.is_empty() {
        out.push_str("\n# Added by wsctl\n");
        for (key, value) in &remaining {
            out.push_str(&format!("{key}={}\n", quote(value)));
        }
    }

    out
}

/// The value of a `KEY=...` line, surrounding quotes removed.
pub fn value(env: &str, key: &str) -> Option<String> {
    env.lines().find_map(|line| {
        let (k, v) = line.trim_start().split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').to_string())
    })
}

fn line_key(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        return None;
    }
    let (key, _) = trimmed.split_once('=')?;
    let key = key.trim();
    (!key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')).then_some(key)
}

fn quote(value: &str) -> String {
    if value.is_empty()
        || value
            .chars()
            .any(|c| c.is_whitespace() || "|#\"'".contains(c))
    {
        format!("\"{}\"", value.replace('"', "\\\""))
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn replaces_values_and_keeps_comments() {
        let template = "# comment\nAPP_URL=http://localhost\nDB_HOST=mysql\n";
        let out = patch(template, &values(&[("APP_URL", "https://example.com")]));
        assert_eq!(
            out,
            "# comment\nAPP_URL=https://example.com\nDB_HOST=mysql\n"
        );
    }

    #[test]
    fn quotes_values_with_spaces_and_appends_missing_keys() {
        let template = "CONTACT_EMAIL=\"a@b.c | X\"\n";
        let out = patch(
            template,
            &values(&[("CONTACT_EMAIL", "me@x.y | Me"), ("NEW_KEY", "1")]),
        );
        assert!(out.contains("CONTACT_EMAIL=\"me@x.y | Me\"\n"));
        assert!(out.contains("# Added by wsctl\nNEW_KEY=1\n"));
    }

    #[test]
    fn value_reads_plain_and_quoted_entries() {
        let env = "# comment\nAPP_DOMAIN=example.com\nWS_DOMAIN=\"ws.example.com\"\n";
        assert_eq!(value(env, "APP_DOMAIN").as_deref(), Some("example.com"));
        assert_eq!(value(env, "WS_DOMAIN").as_deref(), Some("ws.example.com"));
        assert_eq!(value(env, "MISSING"), None);
    }

    #[test]
    fn empty_value_is_quoted_and_placeholder_lines_still_match() {
        let template = "ALLOWED_AFFILIATION_IDS=\nAPP_KEY=<fill in your key here>\n";
        let out = patch(
            template,
            &values(&[("ALLOWED_AFFILIATION_IDS", ""), ("APP_KEY", "base64:abc")]),
        );
        assert_eq!(out, "ALLOWED_AFFILIATION_IDS=\"\"\nAPP_KEY=base64:abc\n");
    }
}
