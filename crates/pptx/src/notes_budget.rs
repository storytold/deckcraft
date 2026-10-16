//! Nonallocating bounded serialization preflight, derived from the existing native notes closure counter.
use crate::{PptxError, notes_codec::invalid};
use serde::{Serialize, ser};
use std::fmt;

#[derive(Debug)]
pub(crate) struct CountError(String);
impl fmt::Display for CountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for CountError {}
impl ser::Error for CountError {
    fn custom<T: fmt::Display>(message: T) -> Self {
        Self(message.to_string())
    }
}
pub(crate) struct Counter {
    bytes: usize,
    limit: usize,
    depth: usize,
}
impl Counter {
    pub(crate) fn new(limit: usize) -> Self {
        Self { bytes: 0, limit, depth: 0 }
    }
    fn enter(&mut self) -> std::result::Result<(), CountError> {
        self.depth = self.depth.checked_add(1).ok_or_else(|| CountError("metadata depth overflow".into()))?;
        if self.depth > 512 {
            return Err(CountError("notes metadata depth limit".into()));
        }
        self.add(32)
    }
    fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    fn add(&mut self, amount: usize) -> std::result::Result<(), CountError> {
        self.bytes = self.bytes.checked_add(amount).ok_or_else(|| CountError("metadata accounting overflow".into()))?;
        if self.bytes > self.limit {
            return Err(CountError("metadata budget exceeded".into()));
        }
        Ok(())
    }
    pub(crate) fn value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), PptxError> {
        value.serialize(&mut *self).map_err(invalid)
    }
}
macro_rules! scalar {
    ($($method:ident($ty:ty)),* $(,)?) => {$ (
        fn $method(self, _: $ty) -> std::result::Result<(), CountError> { self.add(16) }
    )*};
}
impl ser::Serializer for &mut Counter {
    type Ok = ();
    type Error = CountError;
    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeTupleStruct = Self;
    type SerializeTupleVariant = Self;
    type SerializeMap = Self;
    type SerializeStruct = Self;
    type SerializeStructVariant = Self;
    scalar!(
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
        serialize_char(char)
    );
    fn serialize_f32(self, value: f32) -> std::result::Result<(), CountError> {
        self.serialize_f64(f64::from(value))
    }
    fn serialize_f64(self, value: f64) -> std::result::Result<(), CountError> {
        if !value.is_finite() {
            return Err(CountError("nonfinite model value".into()));
        }
        self.add(16)
    }
    fn serialize_str(self, value: &str) -> std::result::Result<(), CountError> {
        self.add(value.len().saturating_add(32))
    }
    fn serialize_bytes(self, value: &[u8]) -> std::result::Result<(), CountError> {
        self.add(value.len().saturating_add(32))
    }
    fn serialize_none(self) -> std::result::Result<(), CountError> {
        self.add(8)
    }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> std::result::Result<(), CountError> {
        value.serialize(self)
    }
    fn serialize_unit(self) -> std::result::Result<(), CountError> {
        self.add(8)
    }
    fn serialize_unit_struct(self, _: &'static str) -> std::result::Result<(), CountError> {
        self.add(8)
    }
    fn serialize_unit_variant(self, _: &'static str, _: u32, _: &'static str) -> std::result::Result<(), CountError> {
        self.add(16)
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(self, _: &'static str, value: &T) -> std::result::Result<(), CountError> {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        value: &T,
    ) -> std::result::Result<(), CountError> {
        self.add(16)?;
        value.serialize(self)
    }
    fn serialize_seq(self, _: Option<usize>) -> std::result::Result<Self, CountError> {
        self.enter()?;
        Ok(self)
    }
    fn serialize_tuple(self, _: usize) -> std::result::Result<Self, CountError> {
        self.enter()?;
        Ok(self)
    }
    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> std::result::Result<Self, CountError> {
        self.enter()?;
        Ok(self)
    }
    fn serialize_tuple_variant(self, _: &'static str, _: u32, _: &'static str, _: usize) -> std::result::Result<Self, CountError> {
        self.enter()?;
        Ok(self)
    }
    fn serialize_map(self, _: Option<usize>) -> std::result::Result<Self, CountError> {
        self.enter()?;
        Ok(self)
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> std::result::Result<Self, CountError> {
        self.enter()?;
        Ok(self)
    }
    fn serialize_struct_variant(self, _: &'static str, _: u32, _: &'static str, _: usize) -> std::result::Result<Self, CountError> {
        self.enter()?;
        Ok(self)
    }
}
macro_rules! sequence {
    ($trait:ident, $method:ident) => {
        impl ser::$trait for &mut Counter {
            type Ok = ();
            type Error = CountError;
            fn $method<T: Serialize + ?Sized>(&mut self, value: &T) -> std::result::Result<(), CountError> {
                self.add(16)?;
                value.serialize(&mut **self)
            }
            fn end(self) -> std::result::Result<(), CountError> {
                self.leave();
                Ok(())
            }
        }
    };
}
sequence!(SerializeSeq, serialize_element);
sequence!(SerializeTuple, serialize_element);
sequence!(SerializeTupleStruct, serialize_field);
sequence!(SerializeTupleVariant, serialize_field);
impl ser::SerializeMap for &mut Counter {
    type Ok = ();
    type Error = CountError;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, value: &T) -> std::result::Result<(), CountError> {
        self.add(16)?;
        value.serialize(&mut **self)
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> std::result::Result<(), CountError> {
        value.serialize(&mut **self)
    }
    fn end(self) -> std::result::Result<(), CountError> {
        self.leave();
        Ok(())
    }
}
macro_rules! structure {
    ($trait:ident) => {
        impl ser::$trait for &mut Counter {
            type Ok = ();
            type Error = CountError;
            fn serialize_field<T: Serialize + ?Sized>(&mut self, _: &'static str, value: &T) -> std::result::Result<(), CountError> {
                self.add(32)?;
                value.serialize(&mut **self)
            }
            fn end(self) -> std::result::Result<(), CountError> {
                self.leave();
                Ok(())
            }
        }
    };
}
structure!(SerializeStruct);
structure!(SerializeStructVariant);
