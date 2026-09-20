use std::fs::File;
use std::path::Path;

use anyhow::{Context, Result};
use parquet::basic::ConvertedType;
use parquet::file::reader::{FileReader, SerializedFileReader};

use crate::scalar::{max_from_statistics, min_from_statistics, scalar_max, scalar_min, Scalar};

/// One column's schema entry: its name and how it's physically/logically
/// typed in the file's own footer.
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnSchema {
    pub name: String,
    pub physical_type: String,
    /// The richer logical/converted type name (`"UTF8"`, `"DATE"`, ...)
    /// when the file actually declares one, distinct from the raw
    /// physical storage type (`BYTE_ARRAY`, `INT32`, ...).
    pub logical_type: Option<String>,
}

/// One row group's own row count, as recorded in the footer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowGroupSummary {
    pub index: usize,
    pub num_rows: i64,
}

/// Per-column statistics, aggregated across every row group in the
/// file. `null_count`/`min`/`max` are `None` only when at least one row
/// group's column chunk has no statistics recorded at all (an honestly
/// unknown value is reported as unknown, not silently as zero or
/// skipped).
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnStats {
    pub name: String,
    pub null_count: Option<u64>,
    pub min: Option<Scalar>,
    pub max: Option<Scalar>,
}

/// Everything this tool reports about one Parquet file, read entirely
/// from the footer metadata - no row data is decoded to produce this.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub num_rows: i64,
    pub num_row_groups: usize,
    pub schema: Vec<ColumnSchema>,
    pub row_groups: Vec<RowGroupSummary>,
    pub columns: Vec<ColumnStats>,
}

/// Reads a Parquet file's footer and builds a [`Report`]: schema, row
/// group count, total row count, and per-column min/max/null-count
/// merged across every row group.
pub fn inspect(path: &Path) -> Result<Report> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let reader = SerializedFileReader::new(file)
        .with_context(|| format!("reading Parquet footer of {}", path.display()))?;

    let metadata = reader.metadata();
    let file_meta = metadata.file_metadata();
    let schema_descr = file_meta.schema_descr();

    let schema: Vec<ColumnSchema> = (0..schema_descr.num_columns())
        .map(|i| {
            let col = schema_descr.column(i);
            let physical_type = col.physical_type().to_string();
            let logical_type = col
                .logical_type_ref()
                .map(|lt| format!("{lt:?}"))
                .or_else(|| {
                    let ct = col.converted_type();
                    (ct != ConvertedType::NONE).then(|| ct.to_string())
                });
            ColumnSchema {
                name: col.name().to_string(),
                physical_type,
                logical_type,
            }
        })
        .collect();

    let num_row_groups = metadata.num_row_groups();
    let mut row_groups = Vec::with_capacity(num_row_groups);

    // running per-column aggregation, indexed the same as `schema`
    let mut null_counts: Vec<Option<u64>> = vec![Some(0); schema.len()];
    let mut mins: Vec<Option<Scalar>> = vec![None; schema.len()];
    let mut maxes: Vec<Option<Scalar>> = vec![None; schema.len()];

    for rg_idx in 0..num_row_groups {
        let rg = metadata.row_group(rg_idx);
        row_groups.push(RowGroupSummary {
            index: rg_idx,
            num_rows: rg.num_rows(),
        });

        for col_idx in 0..rg.num_columns() {
            let col_chunk = rg.column(col_idx);
            match col_chunk.statistics() {
                Some(stats) => {
                    match (null_counts[col_idx], stats.null_count_opt()) {
                        (Some(running), Some(this_rg)) => {
                            null_counts[col_idx] = Some(running + this_rg)
                        }
                        _ => null_counts[col_idx] = None,
                    }

                    if let Some(min) = min_from_statistics(stats) {
                        mins[col_idx] = Some(match mins[col_idx].take() {
                            Some(existing) => scalar_min(existing, min),
                            None => min,
                        });
                    }
                    if let Some(max) = max_from_statistics(stats) {
                        maxes[col_idx] = Some(match maxes[col_idx].take() {
                            Some(existing) => scalar_max(existing, max),
                            None => max,
                        });
                    }
                }
                None => {
                    null_counts[col_idx] = None;
                }
            }
        }
    }

    let columns = schema
        .iter()
        .enumerate()
        .map(|(i, col)| ColumnStats {
            name: col.name.clone(),
            null_count: null_counts[i],
            min: mins[i].take(),
            max: maxes[i].take(),
        })
        .collect();

    Ok(Report {
        num_rows: file_meta.num_rows(),
        num_row_groups,
        schema,
        row_groups,
        columns,
    })
}
