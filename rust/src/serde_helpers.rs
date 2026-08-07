// Strict encoding library for deterministic binary serialization.
//
// SPDX-License-Identifier: Apache-2.0
//
// Copyright 2026 RGB-Tools developers
//
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except
// in compliance with the License. You may obtain a copy of the License at
//
//        http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software distributed under the License
// is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express
// or implied. See the License for the specific language governing permissions and limitations under
// the License.

//! Serde helpers for `amplify` types, to be used with `#[serde(with = "...")]`.
//!
//! `amplify` provides its own serde implementations for these types, but only under its `serde`
//! feature, which also pulls in the unmaintained `paste` crate through `stringly_conversions`.
//! These helpers produce the very same representation, so they can be used instead of enabling
//! `amplify/serde`.

/// (De)serialization of [`amplify::confinement::Confined`] collections, represented as the
/// unconfined collection itself.
pub mod confined {
    use amplify::confinement::{Collection, Confined};
    use serde_crate::de::Error;
    use serde_crate::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<C, S, const MIN: usize, const MAX: usize>(
        confined: &Confined<C, MIN, MAX>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        C: Collection + Serialize,
        S: Serializer,
    {
        confined.as_unconfined().serialize(serializer)
    }

    pub fn deserialize<'de, C, D, const MIN: usize, const MAX: usize>(
        deserializer: D,
    ) -> Result<Confined<C, MIN, MAX>, D::Error>
    where
        C: Collection + Deserialize<'de>,
        D: Deserializer<'de>,
    {
        let collection = C::deserialize(deserializer)?;
        Confined::try_from(collection).map_err(D::Error::custom)
    }
}

/// (De)serialization of [`amplify::Array`] of bytes, represented as a hex string for
/// human-readable formats and as a tuple of bytes otherwise.
pub mod byte_array {
    use core::fmt;

    use amplify::hex::{FromHex, ToHex};
    use amplify::Array;
    use serde_crate::de::{Error, SeqAccess, Visitor};
    use serde_crate::ser::SerializeTuple;
    use serde_crate::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S, const LEN: usize, const REVERSE_STR: bool>(
        array: &Array<u8, LEN, REVERSE_STR>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if serializer.is_human_readable() {
            serializer.serialize_str(&array.to_hex())
        } else {
            let mut ser = serializer.serialize_tuple(LEN)?;
            for byte in array.as_slice() {
                ser.serialize_element(byte)?;
            }
            ser.end()
        }
    }

    pub fn deserialize<'de, D, const LEN: usize, const REVERSE_STR: bool>(
        deserializer: D,
    ) -> Result<Array<u8, LEN, REVERSE_STR>, D::Error>
    where D: Deserializer<'de> {
        if deserializer.is_human_readable() {
            let string = String::deserialize(deserializer)?;
            Array::from_hex(&string).map_err(|_| D::Error::custom("wrong hex data"))
        } else {
            struct ArrayVisitor<const LEN: usize>;

            impl<'de, const LEN: usize> Visitor<'de> for ArrayVisitor<LEN> {
                type Value = [u8; LEN];

                fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                    write!(f, "an array of length {LEN}")
                }

                fn visit_seq<A>(self, mut seq: A) -> Result<[u8; LEN], A::Error>
                where A: SeqAccess<'de> {
                    let mut arr = [0; LEN];
                    for (i, el) in arr.iter_mut().enumerate() {
                        *el = seq
                            .next_element()?
                            .ok_or_else(|| A::Error::invalid_length(i, &self))?;
                    }
                    Ok(arr)
                }
            }

            deserializer
                .deserialize_tuple(LEN, ArrayVisitor::<LEN>)
                .map(Array::<u8, LEN, REVERSE_STR>::from)
        }
    }
}

/// (De)serialization of `amplify` big integers, represented by their big-endian bytes: as a hex
/// string for human-readable formats and as a byte string otherwise.
pub mod big_int {
    use core::fmt;
    use core::marker::PhantomData;

    use amplify::hex::{FromHex, ToHex};
    use serde_crate::de::{Error, Unexpected, Visitor};
    use serde_crate::{Deserializer, Serializer};

    /// Big integers convertible from/into big-endian bytes.
    pub trait BigIntBytes: Copy + Sized {
        /// Big-endian byte representation of the integer.
        type Bytes: AsRef<[u8]>;
        /// Length of [`Self::Bytes`].
        const LEN: usize;

        fn to_be_bytes(self) -> Self::Bytes;
        fn from_be_slice(bytes: &[u8]) -> Option<Self>;
    }

    macro_rules! impl_big_int_bytes {
        ($ty:ty, $len:literal) => {
            impl BigIntBytes for $ty {
                type Bytes = [u8; $len];
                const LEN: usize = $len;

                fn to_be_bytes(self) -> Self::Bytes { <$ty>::to_be_bytes(self) }
                fn from_be_slice(bytes: &[u8]) -> Option<Self> { <$ty>::from_be_slice(bytes).ok() }
            }
        };
    }

    impl_big_int_bytes!(amplify::num::u256, 32);
    impl_big_int_bytes!(amplify::num::u512, 64);
    impl_big_int_bytes!(amplify::num::u1024, 128);
    impl_big_int_bytes!(amplify::num::i256, 32);
    impl_big_int_bytes!(amplify::num::i512, 64);
    impl_big_int_bytes!(amplify::num::i1024, 128);

    pub fn serialize<T, S>(value: &T, serializer: S) -> Result<S::Ok, S::Error>
    where
        T: BigIntBytes,
        S: Serializer,
    {
        let bytes = value.to_be_bytes();
        if serializer.is_human_readable() {
            serializer.serialize_str(&bytes.as_ref().to_hex())
        } else {
            serializer.serialize_bytes(bytes.as_ref())
        }
    }

    pub fn deserialize<'de, T, D>(deserializer: D) -> Result<T, D::Error>
    where
        T: BigIntBytes,
        D: Deserializer<'de>,
    {
        struct BigIntVisitor<T>(PhantomData<T>);

        impl<'de, T: BigIntBytes> Visitor<'de> for BigIntVisitor<T> {
            type Value = T;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "{} bytes or a hex string with {} characters", T::LEN, T::LEN * 2)
            }

            fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
            where E: Error {
                let bytes =
                    Vec::from_hex(s).map_err(|_| E::invalid_value(Unexpected::Str(s), &self))?;
                T::from_be_slice(&bytes).ok_or_else(|| E::invalid_length(bytes.len() * 2, &self))
            }

            fn visit_bytes<E>(self, bytes: &[u8]) -> Result<Self::Value, E>
            where E: Error {
                T::from_be_slice(bytes).ok_or_else(|| E::invalid_length(bytes.len(), &self))
            }
        }

        if deserializer.is_human_readable() {
            deserializer.deserialize_str(BigIntVisitor(PhantomData))
        } else {
            deserializer.deserialize_bytes(BigIntVisitor(PhantomData))
        }
    }
}

/// (De)serialization of `amplify` small integers, represented by the primitive integer they wrap.
///
/// NB: unlike `amplify`, which derives `serde(transparent)` and thus performs no range check,
/// deserialization here rejects values exceeding the maximum of the target type.
pub mod small_int {
    use core::fmt::Display;

    use serde_crate::de::Error;
    use serde_crate::{Deserialize, Deserializer, Serialize, Serializer};

    /// Small integers wrapping a primitive integer.
    pub trait SmallIntPrimitive: Copy + Sized {
        /// The wrapped primitive integer.
        type Inner: Copy + Display + Serialize + for<'de> Deserialize<'de>;

        fn to_inner(self) -> Self::Inner;
        fn from_inner(inner: Self::Inner) -> Option<Self>;
    }

    macro_rules! impl_small_int {
        ($ty:ty, $inner:ty) => {
            impl SmallIntPrimitive for $ty {
                type Inner = $inner;

                fn to_inner(self) -> Self::Inner { self.into() }
                fn from_inner(inner: Self::Inner) -> Option<Self> { Self::try_from(inner).ok() }
            }
        };
    }

    impl_small_int!(amplify::num::u1, u8);
    impl_small_int!(amplify::num::u2, u8);
    impl_small_int!(amplify::num::u3, u8);
    impl_small_int!(amplify::num::u4, u8);
    impl_small_int!(amplify::num::u5, u8);
    impl_small_int!(amplify::num::u6, u8);
    impl_small_int!(amplify::num::u7, u8);
    impl_small_int!(amplify::num::u10, u16);
    impl_small_int!(amplify::num::u12, u16);
    impl_small_int!(amplify::num::u14, u16);
    impl_small_int!(amplify::num::u20, u32);
    impl_small_int!(amplify::num::u24, u32);
    impl_small_int!(amplify::num::u40, u64);
    impl_small_int!(amplify::num::u48, u64);
    impl_small_int!(amplify::num::u56, u64);

    pub fn serialize<T, S>(value: &T, serializer: S) -> Result<S::Ok, S::Error>
    where
        T: SmallIntPrimitive,
        S: Serializer,
    {
        value.to_inner().serialize(serializer)
    }

    pub fn deserialize<'de, T, D>(deserializer: D) -> Result<T, D::Error>
    where
        T: SmallIntPrimitive,
        D: Deserializer<'de>,
    {
        let inner = T::Inner::deserialize(deserializer)?;
        T::from_inner(inner)
            .ok_or_else(|| D::Error::custom(format!("value `{inner}` is out of range")))
    }
}

/// (De)serialization of [`amplify::confinement::Confined`] ASCII strings, represented as a plain
/// string.
///
/// [`confined`] can't be used for them, since `AsciiString` implements serde traits only under the
/// `ascii/serde` feature, enabled by `amplify/serde`.
pub mod confined_ascii {
    use amplify::ascii::AsciiString;
    use amplify::confinement::Confined;
    use serde_crate::de::{Error, Unexpected};
    use serde_crate::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S, const MIN: usize, const MAX: usize>(
        confined: &Confined<AsciiString, MIN, MAX>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(confined.as_str())
    }

    pub fn deserialize<'de, D, const MIN: usize, const MAX: usize>(
        deserializer: D,
    ) -> Result<Confined<AsciiString, MIN, MAX>, D::Error>
    where D: Deserializer<'de> {
        let s = String::deserialize(deserializer)?;
        let ascii = AsciiString::from_ascii(s.as_str())
            .map_err(|_| D::Error::invalid_value(Unexpected::Str(&s), &"an ascii string"))?;
        Confined::try_from(ascii).map_err(D::Error::custom)
    }
}
