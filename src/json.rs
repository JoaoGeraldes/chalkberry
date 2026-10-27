//! A tiny reader and writer for flat JSON objects, `{"key": value, ...}`,
//! where every value is a string, number, `true`/`false` or `null`. That
//! is all the settings file needs, so no JSON library is required.
//!
//! Pure: no Windows APIs and no files. Nested objects and arrays are not
//! supported; text containing them is rejected as a whole.

use std::fmt::Write;
use std::iter::Peekable;
use std::str::Chars;

/// One value in a flat JSON object.
#[derive(Clone, Debug, PartialEq)]
pub enum JsonValue {
    Text(String),
    Number(f64),
    Bool(bool),
    Null,
}

impl JsonValue {
    pub fn as_text(&self) -> Option<&str> {
        match self {
            JsonValue::Text(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_number(&self) -> Option<f64> {
        match self {
            JsonValue::Number(number) => Some(*number),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            JsonValue::Bool(flag) => Some(*flag),
            _ => None,
        }
    }
}

/// Parses `text` as a flat JSON object into its (key, value) pairs, in
/// file order. Returns `None` if it is not one.
pub fn parse_flat_object(text: &str) -> Option<Vec<(String, JsonValue)>> {
    let mut reader = Reader {
        chars: text.chars().peekable(),
    };
    reader.expect('{')?;
    let mut entries = Vec::new();
    if reader.skip_if('}') {
        return reader.is_at_end().then_some(entries);
    }
    loop {
        let key = reader.string()?;
        reader.expect(':')?;
        entries.push((key, reader.value()?));
        if !reader.skip_if(',') {
            reader.expect('}')?;
            return reader.is_at_end().then_some(entries);
        }
    }
}

/// Writes `entries` as a JSON object with one key per line, so the file is
/// easy to read and edit by hand.
pub fn write_flat_object(entries: &[(&str, JsonValue)]) -> String {
    let lines: Vec<String> = entries
        .iter()
        .map(|(key, value)| format!("  {}: {}", quoted(key), value_text(value)))
        .collect();
    format!("{{\n{}\n}}\n", lines.join(",\n"))
}

fn value_text(value: &JsonValue) -> String {
    match value {
        JsonValue::Text(text) => quoted(text),
        JsonValue::Number(number) if number.is_finite() => number.to_string(),
        // JSON has no infinity or NaN.
        JsonValue::Number(_) | JsonValue::Null => "null".to_string(),
        JsonValue::Bool(flag) => flag.to_string(),
    }
}

/// `text` in quotes, with the characters JSON requires escaped (Windows
/// paths are full of backslashes).
fn quoted(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len() + 2);
    escaped.push('"');
    for character in text.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            control if control.is_control() => {
                let _ = write!(escaped, "\\u{:04x}", u32::from(control));
            }
            other => escaped.push(other),
        }
    }
    escaped.push('"');
    escaped
}

/// Reads JSON tokens one character at a time.
struct Reader<'a> {
    chars: Peekable<Chars<'a>>,
}

impl Reader<'_> {
    fn skip_whitespace(&mut self) {
        while self.chars.next_if(|c| c.is_whitespace()).is_some() {}
    }

    /// Consumes `expected` (after any whitespace) if it comes next.
    fn skip_if(&mut self, expected: char) -> bool {
        self.skip_whitespace();
        self.chars.next_if_eq(&expected).is_some()
    }

    fn expect(&mut self, expected: char) -> Option<()> {
        self.skip_if(expected).then_some(())
    }

    fn is_at_end(&mut self) -> bool {
        self.skip_whitespace();
        self.chars.peek().is_none()
    }

    fn value(&mut self) -> Option<JsonValue> {
        self.skip_whitespace();
        match self.chars.peek()? {
            '"' => self.string().map(JsonValue::Text),
            't' => self.word("true").map(|()| JsonValue::Bool(true)),
            'f' => self.word("false").map(|()| JsonValue::Bool(false)),
            'n' => self.word("null").map(|()| JsonValue::Null),
            _ => self.number().map(JsonValue::Number),
        }
    }

    fn word(&mut self, word: &str) -> Option<()> {
        word.chars()
            .all(|expected| self.chars.next() == Some(expected))
            .then_some(())
    }

    fn number(&mut self) -> Option<f64> {
        let mut number_text = String::new();
        while let Some(character) = self
            .chars
            .next_if(|c| c.is_ascii_digit() || "+-.eE".contains(*c))
        {
            number_text.push(character);
        }
        number_text.parse().ok()
    }

    fn string(&mut self) -> Option<String> {
        self.expect('"')?;
        let mut text = String::new();
        loop {
            match self.chars.next()? {
                '"' => return Some(text),
                '\\' => text.push(self.escaped_character()?),
                // JSON does not allow raw control characters in strings.
                control if control.is_control() => return None,
                other => text.push(other),
            }
        }
    }

    /// The character after a backslash.
    fn escaped_character(&mut self) -> Option<char> {
        let character = match self.chars.next()? {
            '"' => '"',
            '\\' => '\\',
            '/' => '/',
            'b' => '\u{8}',
            'f' => '\u{c}',
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            'u' => self.unicode_escape()?,
            _ => return None,
        };
        Some(character)
    }

    /// The four hex digits of `\uXXXX`. Halves of surrogate pairs (used for
    /// characters such as emoji) become U+FFFD; the writer never makes them.
    fn unicode_escape(&mut self) -> Option<char> {
        let hex: String = (0..4).map_while(|_| self.chars.next()).collect();
        if hex.len() != 4 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let code = u32::from_str_radix(&hex, 16).ok()?;
        Some(char::from_u32(code).unwrap_or(char::REPLACEMENT_CHARACTER))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(text: &str) -> Vec<(String, JsonValue)> {
        parse_flat_object(text).expect("valid flat object")
    }

    #[test]
    fn reads_every_kind_of_value() {
        let entries = parsed(r#"{ "a": "hi", "b": -1.5e2, "c": true, "d": false, "e": null }"#);
        let values: Vec<JsonValue> = entries.into_iter().map(|(_, value)| value).collect();
        assert_eq!(
            values,
            [
                JsonValue::Text("hi".into()),
                JsonValue::Number(-150.0),
                JsonValue::Bool(true),
                JsonValue::Bool(false),
                JsonValue::Null,
            ]
        );
    }

    #[test]
    fn empty_object_has_no_entries() {
        assert!(parsed(" { } ").is_empty());
    }

    #[test]
    fn windows_path_survives_write_and_read() {
        let path = r#"C:\Users\João\Pictures\"Pen" Marker"#;
        let text = write_flat_object(&[("folder", JsonValue::Text(path.into()))]);
        assert_eq!(parsed(&text)[0].1.as_text(), Some(path));
    }

    #[test]
    fn unicode_escape_is_decoded() {
        assert_eq!(
            parsed(r#"{"name": "Jo\u00e3o"}"#)[0].1.as_text(),
            Some("João")
        );
    }

    #[test]
    fn rejects_nested_objects_and_arrays() {
        assert_eq!(parse_flat_object(r#"{"pen": {"width": 3}}"#), None);
        assert_eq!(parse_flat_object(r#"{"colors": [1, 2]}"#), None);
    }

    #[test]
    fn rejects_broken_text() {
        assert_eq!(parse_flat_object(r#"{"a": 1,}"#), None);
        assert_eq!(parse_flat_object(r#"{"a": 1} extra"#), None);
        assert_eq!(parse_flat_object(r#"{"a" 1}"#), None);
        assert_eq!(parse_flat_object(r#"{"a": "unclosed}"#), None);
        assert_eq!(parse_flat_object(""), None);
    }

    #[test]
    fn whole_numbers_are_written_without_decimals() {
        let text = write_flat_object(&[("width", JsonValue::Number(10.0))]);
        assert_eq!(text, "{\n  \"width\": 10\n}\n");
    }
}
