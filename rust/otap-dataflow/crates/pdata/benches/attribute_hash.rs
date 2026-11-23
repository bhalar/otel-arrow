// Copyright The OpenTelemetry Authors
// SPDX-License-Identifier: Apache-2.0

//! Measures attribute hashing across dictionary cardinalities, with reference-output checks.

use std::{hint::black_box, sync::Arc};

use arrow::{
    array::{Array, ArrayRef, DictionaryArray, RecordBatch, StringArray, UInt8Array, UInt16Array},
    compute::cast,
    datatypes::{DataType, Field, Schema, UInt8Type, UInt16Type},
};
use criterion::{Criterion, criterion_group, criterion_main};
use otel_arrow_dfe_pdata::{
    otap::transform::{
        AttributesTransform, HashSpec, HashTransform, transform_attributes_with_stats,
    },
    otlp::attributes::AttributeValueType,
    schema::{FieldExt, consts},
};
use sha2::{Digest, Sha256};

const ROWS: usize = 10_000;
const NULL_INTERVAL: usize = 97;
const HASH_KEY: &str = "client.address";
const HASH_SALT: &str = "demo-salt";
const SCENARIOS: &[(&str, usize)] = &[
    ("u16", 50),
    ("u16", 500),
    ("u16", 5000),
    ("u16", ROWS),
    ("u8", 50),
    ("plain", 500),
];

fn bench_hash(criterion: &mut Criterion) {
    let rows = ROWS;
    let mut group = criterion.benchmark_group("attribute_hash");
    let _ = group.throughput(criterion::Throughput::Elements(rows as u64));
    for &(encoding, distinct) in SCENARIOS {
        let values: Vec<_> = (0..rows)
            .map(|row| format!("client-{:08}", row % distinct))
            .collect();
        let strings: Vec<_> = values
            .iter()
            .enumerate()
            .map(|(row, value)| (row % NULL_INTERVAL != 0).then_some(value.as_str()))
            .collect();
        let column: ArrayRef = match encoding {
            "u8" => Arc::new(DictionaryArray::<UInt8Type>::from_iter(
                strings.iter().copied(),
            )),
            "u16" => Arc::new(DictionaryArray::<UInt16Type>::from_iter(
                strings.iter().copied(),
            )),
            _ => Arc::new(StringArray::from(strings.clone())),
        };
        let input = RecordBatch::try_new(
            Arc::new(Schema::new(vec![
                Field::new(consts::PARENT_ID, DataType::UInt16, false).with_plain_encoding(),
                Field::new(consts::ATTRIBUTE_TYPE, DataType::UInt8, false),
                Field::new(consts::ATTRIBUTE_KEY, DataType::Utf8, false),
                Field::new(consts::ATTRIBUTE_STR, column.data_type().clone(), true),
            ])),
            vec![
                Arc::new(UInt16Array::from_iter_values(0..rows as u16)),
                Arc::new(UInt8Array::from(vec![AttributeValueType::Str as u8; rows])),
                Arc::new(StringArray::from(vec![HASH_KEY; rows])),
                column,
            ],
        )
        .expect("valid synthetic attribute batch");
        let parents = Arc::new(UInt16Array::new_null(0)) as ArrayRef;
        let transform = AttributesTransform::default().with_hash(HashTransform::new(
            [(HASH_KEY.into(), HashSpec::sha256(HASH_SALT.into()))].into(),
        ));
        let (output, stats) =
            transform_attributes_with_stats(&input, &parents, &transform).expect("hash fixture");
        let actual = cast(
            output
                .column_by_name(consts::ATTRIBUTE_STR)
                .expect("string values column"),
            &DataType::Utf8,
        )
        .expect("decode output dictionary");
        let actual = actual
            .as_any()
            .downcast_ref::<StringArray>()
            .expect("UTF-8 output");
        assert_eq!(
            stats.hashed_entries,
            strings.iter().flatten().count() as u64
        );
        for (row, value) in strings.iter().enumerate() {
            if let Some(value) = value {
                let expected =
                    hex::encode(Sha256::digest(format!("{HASH_SALT}{value}").as_bytes()));
                assert_eq!(actual.value(row), expected);
            } else {
                assert!(actual.is_null(row));
            }
        }
        let _ = group.bench_function(format!("{encoding}/{distinct}/{rows}"), |bencher| {
            bencher.iter(|| {
                black_box(
                    transform_attributes_with_stats(
                        black_box(&input),
                        black_box(&parents),
                        black_box(&transform),
                    )
                    .expect("hash benchmark batch"),
                )
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_hash);
criterion_main!(benches);
