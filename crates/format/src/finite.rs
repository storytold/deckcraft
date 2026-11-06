//! A serde serializer adapter that writes non-finite floats as 0.
//!
//! serde_json writes NaN and ±infinity as `null`, which doesn't read back as a number: a single
//! NaN anywhere in the model (a rotation, a crop, a stroke width) made a saved file fail to open
//! as "damaged". [`Finite`] wraps any `Serialize` value and replaces those floats on the way out;
//! everything else passes through unchanged.

use serde::ser::{self, Serialize, Serializer};

/// `T`, serialized with non-finite floats written as 0.
pub struct Finite<'a, T: ?Sized>(pub &'a T);

impl<T: ?Sized + Serialize> Serialize for Finite<'_, T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(Wrap(s))
    }
}

struct Wrap<S>(S);

macro_rules! pass {
    ($($m:ident($t:ty)),* $(,)?) => {
        $(fn $m(self, v: $t) -> Result<S::Ok, S::Error> { self.0.$m(v) })*
    };
}

impl<S: Serializer> Serializer for Wrap<S> {
    type Ok = S::Ok;
    type Error = S::Error;
    type SerializeSeq = Wrap<S::SerializeSeq>;
    type SerializeTuple = Wrap<S::SerializeTuple>;
    type SerializeTupleStruct = Wrap<S::SerializeTupleStruct>;
    type SerializeTupleVariant = Wrap<S::SerializeTupleVariant>;
    type SerializeMap = Wrap<S::SerializeMap>;
    type SerializeStruct = Wrap<S::SerializeStruct>;
    type SerializeStructVariant = Wrap<S::SerializeStructVariant>;

    pass!(
        serialize_bool(bool),
        serialize_i8(i8),
        serialize_i16(i16),
        serialize_i32(i32),
        serialize_i64(i64),
        serialize_i128(i128),
        serialize_u8(u8),
        serialize_u16(u16),
        serialize_u32(u32),
        serialize_u64(u64),
        serialize_u128(u128),
        serialize_char(char),
        serialize_str(&str),
        serialize_bytes(&[u8]),
    );

    fn serialize_f32(self, v: f32) -> Result<S::Ok, S::Error> {
        self.0.serialize_f32(if v.is_finite() { v } else { 0.0 })
    }
    fn serialize_f64(self, v: f64) -> Result<S::Ok, S::Error> {
        self.0.serialize_f64(if v.is_finite() { v } else { 0.0 })
    }
    fn serialize_none(self) -> Result<S::Ok, S::Error> {
        self.0.serialize_none()
    }
    fn serialize_some<T: ?Sized + Serialize>(self, v: &T) -> Result<S::Ok, S::Error> {
        self.0.serialize_some(&Finite(v))
    }
    fn serialize_unit(self) -> Result<S::Ok, S::Error> {
        self.0.serialize_unit()
    }
    fn serialize_unit_struct(self, name: &'static str) -> Result<S::Ok, S::Error> {
        self.0.serialize_unit_struct(name)
    }
    fn serialize_unit_variant(self, name: &'static str, i: u32, variant: &'static str) -> Result<S::Ok, S::Error> {
        self.0.serialize_unit_variant(name, i, variant)
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(self, name: &'static str, v: &T) -> Result<S::Ok, S::Error> {
        self.0.serialize_newtype_struct(name, &Finite(v))
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(self, name: &'static str, i: u32, variant: &'static str, v: &T) -> Result<S::Ok, S::Error> {
        self.0.serialize_newtype_variant(name, i, variant, &Finite(v))
    }
    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq, S::Error> {
        self.0.serialize_seq(len).map(Wrap)
    }
    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, S::Error> {
        self.0.serialize_tuple(len).map(Wrap)
    }
    fn serialize_tuple_struct(self, name: &'static str, len: usize) -> Result<Self::SerializeTupleStruct, S::Error> {
        self.0.serialize_tuple_struct(name, len).map(Wrap)
    }
    fn serialize_tuple_variant(self, name: &'static str, i: u32, variant: &'static str, len: usize) -> Result<Self::SerializeTupleVariant, S::Error> {
        self.0.serialize_tuple_variant(name, i, variant, len).map(Wrap)
    }
    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap, S::Error> {
        self.0.serialize_map(len).map(Wrap)
    }
    fn serialize_struct(self, name: &'static str, len: usize) -> Result<Self::SerializeStruct, S::Error> {
        self.0.serialize_struct(name, len).map(Wrap)
    }
    fn serialize_struct_variant(
        self,
        name: &'static str,
        i: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStructVariant, S::Error> {
        self.0.serialize_struct_variant(name, i, variant, len).map(Wrap)
    }
    fn is_human_readable(&self) -> bool {
        self.0.is_human_readable()
    }
}

macro_rules! compound {
    ($tr:ident, $m:ident) => {
        impl<S: ser::$tr> ser::$tr for Wrap<S> {
            type Ok = S::Ok;
            type Error = S::Error;
            fn $m<T: ?Sized + Serialize>(&mut self, v: &T) -> Result<(), S::Error> {
                self.0.$m(&Finite(v))
            }
            fn end(self) -> Result<S::Ok, S::Error> {
                self.0.end()
            }
        }
    };
}
compound!(SerializeSeq, serialize_element);
compound!(SerializeTuple, serialize_element);
compound!(SerializeTupleStruct, serialize_field);
compound!(SerializeTupleVariant, serialize_field);

impl<S: ser::SerializeMap> ser::SerializeMap for Wrap<S> {
    type Ok = S::Ok;
    type Error = S::Error;
    fn serialize_key<T: ?Sized + Serialize>(&mut self, k: &T) -> Result<(), S::Error> {
        self.0.serialize_key(&Finite(k))
    }
    fn serialize_value<T: ?Sized + Serialize>(&mut self, v: &T) -> Result<(), S::Error> {
        self.0.serialize_value(&Finite(v))
    }
    fn end(self) -> Result<S::Ok, S::Error> {
        self.0.end()
    }
}

macro_rules! fields {
    ($tr:ident) => {
        impl<S: ser::$tr> ser::$tr for Wrap<S> {
            type Ok = S::Ok;
            type Error = S::Error;
            fn serialize_field<T: ?Sized + Serialize>(&mut self, key: &'static str, v: &T) -> Result<(), S::Error> {
                self.0.serialize_field(key, &Finite(v))
            }
            fn skip_field(&mut self, key: &'static str) -> Result<(), S::Error> {
                self.0.skip_field(key)
            }
            fn end(self) -> Result<S::Ok, S::Error> {
                self.0.end()
            }
        }
    };
}
fields!(SerializeStruct);
fields!(SerializeStructVariant);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_finite_floats_are_written_as_zero_and_nothing_else_changes() {
        #[derive(serde::Serialize)]
        struct T {
            a: f64,
            b: Option<f64>,
            c: Vec<(f32, f64)>,
            d: Option<String>,
            e: std::collections::BTreeMap<String, f64>,
        }
        let t = T { a: f64::NAN, b: Some(f64::INFINITY), c: vec![(f32::NEG_INFINITY, 1.5)], d: None, e: [("k".to_string(), f64::NAN)].into() };
        let s = serde_json::to_string(&Finite(&t)).unwrap_or_default();
        assert_eq!(s, r#"{"a":0.0,"b":0.0,"c":[[0.0,1.5]],"d":null,"e":{"k":0.0}}"#);
    }
}
