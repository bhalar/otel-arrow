// Copyright The OpenTelemetry Authors
// SPDX-License-Identifier: Apache-2.0

use arrow::array::{ArrayRef, DictionaryArray, StringBuilder};
use arrow::datatypes::{DataType, UInt8Type, UInt16Type};
use sha2::{Digest, Sha256};

use super::append_sha256_hex_lower;
use crate::arrays::NullableArrayAccessor;
use crate::otlp::attributes::AttributeValueType;
use crate::otlp::common::AnyValueArrays;

pub(super) struct DictionaryHashCache {
    keys: Vec<Option<usize>>,
    digests: Vec<Option<[u8; 32]>>,
}

impl DictionaryHashCache {
    pub(super) fn new(column: &ArrayRef) -> Option<Self> {
        let keys: Vec<_> = match column.data_type() {
            DataType::Dictionary(key, _) if key.as_ref() == &DataType::UInt8 => column
                .as_any()
                .downcast_ref::<DictionaryArray<UInt8Type>>()?
                .keys()
                .iter()
                .map(|key| key.map(usize::from))
                .collect(),
            DataType::Dictionary(key, _) if key.as_ref() == &DataType::UInt16 => column
                .as_any()
                .downcast_ref::<DictionaryArray<UInt16Type>>()?
                .keys()
                .iter()
                .map(|key| key.map(usize::from))
                .collect(),
            _ => return None,
        };
        let digests = vec![
            None;
            keys.iter()
                .flatten()
                .copied()
                .max()
                .map_or(0, |key| key + 1)
        ];
        Some(Self { keys, digests })
    }

    pub(super) fn append(
        &mut self,
        builder: &mut StringBuilder,
        arrays: &AnyValueArrays<'_>,
        row: usize,
        salted_hasher: &Sha256,
    ) -> bool {
        if arrays.attr_type.value_at(row) != Some(AttributeValueType::Str as u8) {
            return false;
        }
        let Some(key) = self.keys[row] else {
            return false;
        };
        let Some(value) = arrays
            .attr_str
            .as_ref()
            .and_then(|column| column.str_at(row))
        else {
            return false;
        };
        let digest = self.digests[key].get_or_insert_with(|| {
            let mut hasher = salted_hasher.clone();
            hasher.update(value.as_bytes());
            hasher.finalize().into()
        });
        append_sha256_hex_lower(builder, digest);
        true
    }
}
