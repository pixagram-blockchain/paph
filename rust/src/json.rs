//! A JSON writer that produces exactly what JavaScript's `JSON.stringify`
//! produces for the same value.
//!
//! The comparator report is the one place the two engines meet as TEXT rather
//! than as bytes, so "the same report" has to mean the same characters: the
//! same key order (insertion order, as JavaScript objects keep it), the same
//! string escapes, and the same spelling of every number — including the one
//! float the report carries, a recovered scale, which JavaScript writes with
//! the shortest digits that round-trip and switches to exponent form outside
//! [1e-6, 1e21).  No dependency: the shape is fixed and small, and a parser is
//! not needed at all.

/// A JSON value with insertion-ordered objects.
#[derive(Clone, Debug, PartialEq)]
pub enum J {
    Null,
    Bool(bool),
    Int(i64),
    Num(f64),
    Str(String),
    S(&'static str),
    Arr(Vec<J>),
    Obj(Vec<(&'static str, J)>),
}

impl J {
    pub fn to_string(&self) -> String {
        let mut s = String::with_capacity(256);
        self.write(&mut s);
        s
    }

    pub fn write(&self, out: &mut String) {
        match self {
            J::Null => out.push_str("null"),
            J::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            J::Int(v) => {
                use std::fmt::Write;
                let _ = write!(out, "{}", v);
            }
            J::Num(v) => js_number(*v, out),
            J::Str(s) => js_string(s, out),
            J::S(s) => js_string(s, out),
            J::Arr(v) => {
                out.push('[');
                for (i, x) in v.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    x.write(out);
                }
                out.push(']');
            }
            J::Obj(v) => {
                out.push('{');
                for (i, (k, x)) in v.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    js_string(k, out);
                    out.push(':');
                    x.write(out);
                }
                out.push('}');
            }
        }
    }

    /// Replace the value under `key`, keeping its position — what assigning
    /// to an existing property does in JavaScript.
    pub fn set(&mut self, key: &'static str, v: J) {
        if let J::Obj(fields) = self {
            if let Some(f) = fields.iter_mut().find(|f| f.0 == key) {
                f.1 = v;
            } else {
                fields.push((key, v));
            }
        }
    }

    pub fn get(&self, key: &str) -> Option<&J> {
        match self {
            J::Obj(fields) => fields.iter().find(|f| f.0 == key).map(|f| &f.1),
            _ => None,
        }
    }
}

impl From<bool> for J {
    fn from(v: bool) -> J {
        J::Bool(v)
    }
}
impl From<i64> for J {
    fn from(v: i64) -> J {
        J::Int(v)
    }
}
impl From<i32> for J {
    fn from(v: i32) -> J {
        J::Int(v as i64)
    }
}
impl From<usize> for J {
    fn from(v: usize) -> J {
        J::Int(v as i64)
    }
}
impl From<u8> for J {
    fn from(v: u8) -> J {
        J::Int(v as i64)
    }
}
impl From<u16> for J {
    fn from(v: u16) -> J {
        J::Int(v as i64)
    }
}
impl From<&'static str> for J {
    fn from(v: &'static str) -> J {
        J::S(v)
    }
}
impl From<String> for J {
    fn from(v: String) -> J {
        J::Str(v)
    }
}
impl<T: Into<J>> From<Option<T>> for J {
    fn from(v: Option<T>) -> J {
        match v {
            Some(x) => x.into(),
            None => J::Null,
        }
    }
}

/// `JSON.stringify` of a string: `"` and `\` escaped, the C0 controls as
/// `\b \f \n \r \t` or `\u00xx`, everything else verbatim.
pub fn js_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                use std::fmt::Write;
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// ECMAScript `Number::toString` (radix 10) as `JSON.stringify` uses it:
/// shortest round-trip digits, decimal layout for exponents in [-7, 21),
/// exponent layout (`1.5e-7`, `1e+21`) outside, `null` for non-finite.
pub fn js_number(v: f64, out: &mut String) {
    if !v.is_finite() {
        out.push_str("null");
        return;
    }
    if v == 0.0 {
        out.push('0');
        return;
    }
    if v < 0.0 {
        out.push('-');
    }
    // Rust's `{:e}` is the shortest round-trip digit string, as JavaScript's
    // is: "d[.ddd]e[-]x"
    let e = format!("{:e}", v.abs());
    let (mant, exp) = e.split_once('e').unwrap();
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i64;
    let n = exp.parse::<i64>().unwrap() + 1;
    if k <= n && n <= 21 {
        out.push_str(&digits);
        for _ in 0..(n - k) {
            out.push('0');
        }
    } else if 0 < n && n <= 21 {
        out.push_str(&digits[..n as usize]);
        out.push('.');
        out.push_str(&digits[n as usize..]);
    } else if -6 < n && n <= 0 {
        out.push_str("0.");
        for _ in 0..(-n) {
            out.push('0');
        }
        out.push_str(&digits);
    } else {
        out.push_str(&digits[..1]);
        if k > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('e');
        out.push(if n - 1 >= 0 { '+' } else { '-' });
        use std::fmt::Write;
        let _ = write!(out, "{}", (n - 1).abs());
    }
}

/// An object literal: `obj![ "a" => 1, "b" => true ]`.
#[macro_export]
macro_rules! obj {
    ($($k:literal => $v:expr),* $(,)?) => {
        $crate::json::J::Obj(vec![$(($k, $crate::json::J::from($v))),*])
    };
}

impl From<J> for String {
    fn from(j: J) -> String {
        j.to_string()
    }
}

/// Arrays of anything convertible.
pub fn arr<T: Into<J>, I: IntoIterator<Item = T>>(it: I) -> J {
    J::Arr(it.into_iter().map(|x| x.into()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn num(v: f64) -> String {
        let mut s = String::new();
        js_number(v, &mut s);
        s
    }

    /// The spellings JavaScript gives, checked against `String(x)` in node.
    #[test]
    fn numbers_are_spelled_as_javascript_spells_them() {
        let cases: &[(f64, &str)] = &[
            (1.0, "1"),
            (0.5, "0.5"),
            (0.968475341796875, "0.968475341796875"),
            (1.0 / 0.968475341796875, "1.032550811406964"),
            (1.0 / 65536.0, "0.0000152587890625"),
            (65536.0, "65536"),
            (123456789012345680000.0, "123456789012345680000"),
            (1e21, "1e+21"),
            (1.5e-7, "1.5e-7"),
            (1e-7, "1e-7"),
            (0.000001, "0.000001"),
            (-2.25, "-2.25"),
            (0.1 + 0.2, "0.30000000000000004"),
            (100.0, "100"),
            (1.0 / 3.0, "0.3333333333333333"),
            (5e-324, "5e-324"),
            (1.7976931348623157e308, "1.7976931348623157e+308"),
        ];
        for &(v, want) in cases {
            assert_eq!(num(v), want, "{v:?}");
        }
        assert_eq!(num(f64::NAN), "null");
        assert_eq!(num(-0.0), "0");
    }

    #[test]
    fn strings_and_structures() {
        let v = obj![
            "a" => 1i64,
            "b" => "x\"y\\z\n\u{01}—",
            "c" => J::Arr(vec![J::Null, J::Bool(false)]),
            "d" => None::<i64>,
        ];
        assert_eq!(v.to_string(), "{\"a\":1,\"b\":\"x\\\"y\\\\z\\n\\u0001—\",\"c\":[null,false],\"d\":null}");
        let mut w = v.clone();
        w.set("a", J::Int(2));
        w.set("e", J::Int(3));
        assert_eq!(w.to_string(), "{\"a\":2,\"b\":\"x\\\"y\\\\z\\n\\u0001—\",\"c\":[null,false],\"d\":null,\"e\":3}");
    }
}
