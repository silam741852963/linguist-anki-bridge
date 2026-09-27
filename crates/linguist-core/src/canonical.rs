//! RFC 8785 bytes and strict input parsing; digests are domain separated.
use serde::{
    Deserialize, Serialize,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use serde_json::{Map, Value};
use sha2::{Digest as _, Sha256};
use std::fmt;

pub const FORMAT: &str = "lab-jcs-v1";
const SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Debug)]
pub struct ContractError(pub String);
impl fmt::Display for ContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ContractError {}
impl From<serde_json::Error> for ContractError {
    fn from(e: serde_json::Error) -> Self {
        Self(e.to_string())
    }
}

struct Strict(Value);
impl<'de> Deserialize<'de> for Strict {
    fn deserialize<D: de::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Strict;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("strict JSON without duplicate keys")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Strict, E> {
                if v.unsigned_abs() > SAFE_INTEGER {
                    return Err(E::custom("integer exceeds JCS safe range; use string ID"));
                }
                Ok(Strict(v.into()))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Strict, E> {
                if v > SAFE_INTEGER {
                    return Err(E::custom("integer exceeds JCS safe range; use string ID"));
                }
                Ok(Strict(v.into()))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Strict, E> {
                serde_json::Number::from_f64(v)
                    .map(|n| Strict(n.into()))
                    .ok_or_else(|| E::custom("non-finite number"))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Strict, E> {
                Ok(Strict(Value::Null))
            }
            fn visit_none<E: de::Error>(self) -> Result<Strict, E> {
                Ok(Strict(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Strict, A::Error> {
                let mut values = Vec::new();
                while let Some(Strict(v)) = a.next_element()? {
                    values.push(v)
                }
                Ok(Strict(values.into()))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Strict, A::Error> {
                let mut map = Map::new();
                while let Some(k) = a.next_key::<String>()? {
                    if map.contains_key(&k) {
                        return Err(de::Error::custom(format!("duplicate key: {k}")));
                    }
                    let Strict(v) = a.next_value()?;
                    map.insert(k, v);
                }
                Ok(Strict(map.into()))
            }
        }
        d.deserialize_any(V)
    }
}

pub fn parse<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, ContractError> {
    let Strict(value): Strict = serde_json::from_slice(bytes)?;
    Ok(serde_json::from_value(value)?)
}

pub fn bytes<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, ContractError> {
    // Serialize first with JCS's rejecting float serializer, then check unsafe integers.
    let data = serde_jcs::to_vec(value)?;
    let _: Value = parse(&data)?;
    Ok(data)
}
pub fn digest<T: Serialize + ?Sized>(domain: &str, value: &T) -> Result<String, ContractError> {
    if domain.is_empty() || domain.contains('\0') {
        return Err(ContractError("invalid digest domain".into()));
    }
    let mut h = Sha256::new();
    h.update(FORMAT);
    h.update([0]);
    h.update(domain);
    h.update([0]);
    h.update(bytes(value)?);
    Ok(format!("{FORMAT}:{domain}:{:x}", h.finalize()))
}
pub fn asset_digest(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}
