//! Plain JSON form of [`Value`] (feature `serde`).
//!
//! Scalars, lists, and objects map to their JSON counterparts, so `"Titanium"`,
//! `5`, `2.5`, `true`, `null`, `[1, 2]`, and `{"grade": "Ti"}` are values. A
//! bounding box is the single-key object `{"Aabb": {"min": [..], "max": [..]}}`.
//! Any other object with a single `Aabb` key is therefore read as a box, not a map.
//! Integers without a fraction or exponent are [`Value::Int`]; numbers with one are
//! [`Value::Float`].

use alloc::{collections::BTreeMap, string::String, vec::Vec};
use core::fmt;

use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{MapAccess, SeqAccess, Visitor},
    ser::{SerializeMap, SerializeSeq},
};

use crate::source::{Aabb, Value};

impl Serialize for Value {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Value::Null => s.serialize_unit(),
            Value::Bool(b) => s.serialize_bool(*b),
            Value::Int(i) => s.serialize_i64(*i),
            Value::Float(f) => s.serialize_f64(*f),
            Value::Str(v) => s.serialize_str(v),
            Value::Aabb(b) => {
                let mut m = s.serialize_map(Some(1))?;
                m.serialize_entry("Aabb", b)?;
                m.end()
            }
            Value::List(items) => {
                let mut seq = s.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            Value::Map(map) => {
                let mut m = s.serialize_map(Some(map.len()))?;
                for (k, v) in map {
                    m.serialize_entry(k, v)?;
                }
                m.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_any(ValueVisitor)
    }
}

struct ValueVisitor;

impl<'de> Visitor<'de> for ValueVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a null, boolean, number, string, array, or object")
    }

    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_none<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<Value, D::Error> {
        d.deserialize_any(ValueVisitor)
    }

    fn visit_bool<E>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }

    fn visit_i64<E>(self, v: i64) -> Result<Value, E> {
        Ok(Value::Int(v))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Value, E> {
        Ok(match i64::try_from(v) {
            Ok(i) => Value::Int(i),
            Err(_) => Value::Float(v as f64),
        })
    }

    fn visit_f64<E>(self, v: f64) -> Result<Value, E> {
        Ok(Value::Float(v))
    }

    fn visit_str<E>(self, v: &str) -> Result<Value, E> {
        Ok(Value::Str(String::from(v)))
    }

    fn visit_string<E>(self, v: String) -> Result<Value, E> {
        Ok(Value::Str(v))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element::<Value>()? {
            items.push(item);
        }
        Ok(Value::List(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut entries: BTreeMap<String, Value> = BTreeMap::new();
        while let Some((k, v)) = map.next_entry::<String, Value>()? {
            entries.insert(k, v);
        }
        if entries.len() == 1
            && let Some(bounds) = entries.get("Aabb").and_then(aabb_from)
        {
            return Ok(Value::Aabb(bounds));
        }
        Ok(Value::Map(entries))
    }
}

/// Reads `{"min": [x, y, z], "max": [x, y, z]}` as a box. Numbers may be ints or floats.
fn aabb_from(v: &Value) -> Option<Aabb> {
    let Value::Map(m) = v else { return None };
    let corner = |key: &str| -> Option<[f64; 3]> {
        let Value::List(items) = m.get(key)? else {
            return None;
        };
        let [a, b, c] = items.as_slice() else {
            return None;
        };
        let num = |v: &Value| match v {
            Value::Int(i) => Some(*i as f64),
            Value::Float(f) => Some(*f),
            _ => None,
        };
        Some([num(a)?, num(b)?, num(c)?])
    };
    if m.len() != 2 {
        return None;
    }
    Some(Aabb::new(corner("min")?, corner("max")?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_json_values_read_as_scalars() {
        let v: Value = serde_json::from_str(r#"["Titanium", 5, 2.5, true, null]"#).unwrap();
        assert_eq!(
            v,
            Value::List(alloc::vec![
                Value::Str("Titanium".into()),
                Value::Int(5),
                Value::Float(2.5),
                Value::Bool(true),
                Value::Null,
            ])
        );
    }

    #[test]
    fn boxes_round_trip_and_maps_stay_maps() {
        let b = Value::Aabb(Aabb::new([0.0, 0.0, 0.0], [1.0, 2.0, 3.0]));
        let json = serde_json::to_string(&b).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&json).unwrap(), b);

        let m: Value = serde_json::from_str(r#"{"grade": "Ti", "rev": 2}"#).unwrap();
        assert!(matches!(m, Value::Map(_)));
    }
}
