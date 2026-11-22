//! Restricted CJ1 data serialization only; no signing or trust primitives.
use super::{error, model::SAFE_INTEGER};
use crate::BridgeError;
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use std::fmt;

struct Unique(Value);
impl<'de> Deserialize<'de> for Unique {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Unique;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("restricted JSON")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Unique, E> {
                Ok(Unique(Value::Bool(v)))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Unique, E> {
                Ok(Unique(Value::Null))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Unique, E> {
                Ok(Unique(Value::String(v.into())))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Unique, E> {
                Ok(Unique(Value::String(v)))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Unique, E> {
                if v.unsigned_abs() > SAFE_INTEGER {
                    return Err(E::custom("integer bound"));
                }
                Ok(Unique(Value::from(v)))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Unique, E> {
                if v > SAFE_INTEGER {
                    return Err(E::custom("integer bound"));
                }
                Ok(Unique(Value::from(v)))
            }
            fn visit_f64<E: de::Error>(self, _: f64) -> Result<Unique, E> {
                Err(E::custom("floats forbidden"))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Unique, A::Error> {
                let mut v = Vec::new();
                while let Some(x) = a.next_element::<Unique>()? {
                    if v.len() == 1000 {
                        return Err(de::Error::custom("array bound"));
                    }
                    v.push(x.0);
                }
                Ok(Unique(Value::Array(v)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Unique, A::Error> {
                let mut v = serde_json::Map::new();
                while let Some(k) = a.next_key::<String>()? {
                    if !k.is_ascii() || v.len() == 1000 || v.contains_key(&k) {
                        return Err(de::Error::custom("object keys"));
                    }
                    v.insert(k, a.next_value::<Unique>()?.0);
                }
                Ok(Unique(Value::Object(v)))
            }
        }
        d.deserialize_any(V)
    }
}
pub fn decode(bytes: &[u8]) -> Result<Value, BridgeError> {
    if bytes.len() > super::model::MAX_FRAME {
        return Err(error("invalid_input"));
    }
    // serde_json otherwise represents lexical -0 as a float. CJ1 normalizes that
    // integer spelling, while -0.0 and exponent spellings must remain forbidden.
    let mut normalized = Vec::with_capacity(bytes.len());
    let (mut i, mut quoted, mut escaped) = (0, false, false);
    while i < bytes.len() {
        let b = bytes[i];
        if !quoted
            && b == b'-'
            && bytes.get(i + 1) == Some(&b'0')
            && bytes.get(i + 2).map_or(true, |c| {
                matches!(c, b',' | b'}' | b']' | b' ' | b'\n' | b'\r' | b'\t')
            })
        {
            i += 1;
            normalized.push(b'0');
        } else {
            normalized.push(b);
            if quoted {
                if escaped {
                    escaped = false
                } else if b == b'\\' {
                    escaped = true
                } else if b == b'"' {
                    quoted = false
                }
            } else if b == b'"' {
                quoted = true
            }
        }
        i += 1;
    }
    let v = serde_json::from_slice::<Unique>(&normalized)
        .map_err(|_| error("invalid_input"))?
        .0;
    canonical(&v)?;
    Ok(v)
}
pub fn canonical(value: &Value) -> Result<String, BridgeError> {
    fn string(s: &str, out: &mut String) {
        out.push('"');
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\x08' => out.push_str("\\b"),
                '\x0c' => out.push_str("\\f"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if (' '..='~').contains(&c) => out.push(c),
                c => {
                    for u in c.encode_utf16(&mut [0; 2]) {
                        out.push_str(&format!("\\u{u:04x}"));
                    }
                }
            }
        }
        out.push('"');
    }
    fn emit(v: &Value, out: &mut String, depth: usize) -> Result<(), BridgeError> {
        if depth > 32 {
            return Err(error("invalid_input"));
        }
        match v {
            Value::Null => out.push_str("null"),
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Value::String(s) => string(s, out),
            Value::Number(n) => {
                if !n.as_i64().is_some_and(|i| i.unsigned_abs() <= SAFE_INTEGER)
                    && !n.as_u64().is_some_and(|i| i <= SAFE_INTEGER)
                {
                    return Err(error("invalid_input"));
                }
                out.push_str(&n.to_string());
            }
            Value::Array(a) => {
                if a.len() > 1000 {
                    return Err(error("invalid_input"));
                }
                out.push('[');
                for (i, x) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',')
                    }
                    emit(x, out, depth + 1)?
                }
                out.push(']');
            }
            Value::Object(m) => {
                if m.len() > 1000 {
                    return Err(error("invalid_input"));
                }
                out.push('{');
                let mut keys: Vec<_> = m.keys().collect();
                keys.sort();
                for (i, k) in keys.iter().enumerate() {
                    if !k.is_ascii() {
                        return Err(error("invalid_input"));
                    }
                    if i > 0 {
                        out.push(',')
                    }
                    string(k, out);
                    out.push(':');
                    emit(&m[*k], out, depth + 1)?
                }
                out.push('}');
            }
        }
        Ok(())
    }
    let mut out = String::new();
    emit(value, &mut out, 0)?;
    if out.len() > super::model::MAX_FRAME {
        return Err(error("invalid_input"));
    }
    Ok(out)
}
