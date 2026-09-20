//! Real Parquet file fixtures, written with `arrow`'s own `ArrowWriter` -
//! genuine footer metadata produced by the same Apache Arrow C++-derived
//! implementation any other Parquet reader would produce, not hand-faked
//! bytes. Only compiled for tests: `parquetpeek` itself never writes
//! Parquet files, only reads them.

use std::path::Path;
use std::sync::Arc;

use arrow::array::{
    ArrayRef, BooleanArray, Float64Array, Int32Array, Int64Array, RecordBatch, StringArray,
};
use arrow::datatypes::{DataType, Field, Schema};
use parquet::arrow::arrow_writer::ArrowWriter;
use parquet::file::properties::WriterProperties;

/// Writes a small single-row-group fixture: 5 rows, 4 columns
/// (`id: Int32`, `name: Utf8` with one null, `score: Float64`,
/// `active: Boolean`), and returns the values so tests can assert
/// against them without duplicating literals.
pub struct BasicFixture {
    pub ids: Vec<i32>,
    pub names: Vec<Option<&'static str>>,
    pub scores: Vec<f64>,
    pub active: Vec<bool>,
}

pub fn write_basic_fixture(path: &Path) -> BasicFixture {
    let fixture = BasicFixture {
        ids: vec![1, 2, 3, 4, 5],
        names: vec![Some("alice"), Some("bob"), None, Some("dave"), Some("eve")],
        scores: vec![10.5, -3.25, 7.0, 42.125, 0.0],
        active: vec![true, false, true, false, true],
    };

    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int32, false),
        Field::new("name", DataType::Utf8, true),
        Field::new("score", DataType::Float64, false),
        Field::new("active", DataType::Boolean, false),
    ]));

    let columns: Vec<ArrayRef> = vec![
        Arc::new(Int32Array::from(fixture.ids.clone())),
        Arc::new(StringArray::from(fixture.names.clone())),
        Arc::new(Float64Array::from(fixture.scores.clone())),
        Arc::new(BooleanArray::from(fixture.active.clone())),
    ];
    let batch = RecordBatch::try_new(schema.clone(), columns).expect("valid record batch");

    let file = std::fs::File::create(path).expect("create fixture file");
    let mut writer = ArrowWriter::try_new(file, schema, None).expect("create arrow parquet writer");
    writer.write(&batch).expect("write record batch");
    writer.close().expect("close parquet writer");

    fixture
}

/// Writes a fixture forced into 3 real row groups (sizes 2, 2, 1) via
/// `max_row_group_row_count`, so aggregation across row groups is
/// exercised against genuinely separate footer entries, not a single
/// one. One column (`code: Int32`) has a null in the first row group
/// and none elsewhere, to exercise null-count summation across groups.
pub struct MultiGroupFixture {
    pub codes: Vec<Option<i32>>,
    pub row_group_sizes: Vec<i64>,
}

pub fn write_multi_row_group_fixture(path: &Path) -> MultiGroupFixture {
    let fixture = MultiGroupFixture {
        codes: vec![Some(30), None, Some(10), Some(50), Some(20)],
        row_group_sizes: vec![2, 2, 1],
    };

    let schema = Arc::new(Schema::new(vec![Field::new("code", DataType::Int32, true)]));

    let columns: Vec<ArrayRef> = vec![Arc::new(Int32Array::from(fixture.codes.clone()))];
    let batch = RecordBatch::try_new(schema.clone(), columns).expect("valid record batch");

    let props = WriterProperties::builder()
        .set_max_row_group_row_count(Some(2))
        .build();

    let file = std::fs::File::create(path).expect("create fixture file");
    let mut writer =
        ArrowWriter::try_new(file, schema, Some(props)).expect("create arrow parquet writer");
    writer.write(&batch).expect("write record batch");
    writer.close().expect("close parquet writer");

    fixture
}

/// Writes a fixture with two columns to exercise edge cases the two
/// fixtures above don't cover: `all_null: Utf8` where every single value
/// is null (statistics are present, but min/max have nothing to report
/// since there are no non-null values), and `plain: Int64` with no
/// logical/converted type annotation at all - Arrow's writer only
/// attaches `LogicalType`/`ConvertedType` metadata when the physical
/// type needs disambiguating (a bare `Int64` doesn't).
pub fn write_edge_case_fixture(path: &Path) {
    let schema = Arc::new(Schema::new(vec![
        Field::new("all_null", DataType::Utf8, true),
        Field::new("plain", DataType::Int64, false),
    ]));

    let all_null: Vec<Option<&str>> = vec![None, None, None];
    let columns: Vec<ArrayRef> = vec![
        Arc::new(StringArray::from(all_null)),
        Arc::new(Int64Array::from(vec![100i64, 200, 300])),
    ];
    let batch = RecordBatch::try_new(schema.clone(), columns).expect("valid record batch");

    let file = std::fs::File::create(path).expect("create fixture file");
    let mut writer = ArrowWriter::try_new(file, schema, None).expect("create arrow parquet writer");
    writer.write(&batch).expect("write record batch");
    writer.close().expect("close parquet writer");
}
