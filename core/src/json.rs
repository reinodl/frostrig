//! A tiny JSON writer, enough for the status document the service sends to the app.

#[derive(Clone, Debug, PartialEq)]
pub enum J {
    Null,
    Bool(bool),
    Int(i64),
    Num(f64),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(&'static str, J)>),
}

impl J {
    pub fn str(s: impl Into<String>) -> J {
        J::Str(s.into())
    }

    pub fn opt<T: Into<J>>(v: Option<T>) -> J {
        v.map(Into::into).unwrap_or(J::Null)
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut String) {
        match self {
            J::Null => out.push_str("null"),
            J::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            J::Int(n) => out.push_str(&n.to_string()),
            J::Num(n) if n.is_finite() => out.push_str(&format!("{}", (n * 100.0).round() / 100.0)),
            J::Num(_) => out.push_str("null"),
            J::Str(s) => escape(s, out),
            J::Arr(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 { out.push(','); }
                    item.write(out);
                }
                out.push(']');
            }
            J::Obj(fields) => {
                out.push('{');
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 { out.push(','); }
                    escape(k, out);
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
        }
    }
}

fn escape(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

impl From<bool> for J { fn from(v: bool) -> J { J::Bool(v) } }
impl From<u8> for J { fn from(v: u8) -> J { J::Int(v as i64) } }
impl From<u32> for J { fn from(v: u32) -> J { J::Int(v as i64) } }
impl From<u64> for J { fn from(v: u64) -> J { J::Int(v as i64) } }
impl From<f64> for J { fn from(v: f64) -> J { J::Num(v) } }
impl From<&str> for J { fn from(v: &str) -> J { J::Str(v.to_owned()) } }
impl From<String> for J { fn from(v: String) -> J { J::Str(v) } }
impl<T: Into<J>> From<Vec<T>> for J {
    fn from(v: Vec<T>) -> J { J::Arr(v.into_iter().map(Into::into).collect()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_nested_values_and_escapes() {
        let doc = J::Obj(vec![
            ("a", J::Int(1)),
            ("b", J::str("x\"y\n")),
            ("c", J::Arr(vec![J::Bool(true), J::Null, J::Num(1.234)])),
        ]);
        assert_eq!(doc.render(), r#"{"a":1,"b":"x\"y\n","c":[true,null,1.23]}"#);
    }
}
