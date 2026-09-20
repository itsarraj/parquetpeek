pub mod inspect;
pub mod scalar;

#[cfg(test)]
mod fixtures;

pub use inspect::{inspect, ColumnSchema, ColumnStats, Report, RowGroupSummary};
pub use scalar::Scalar;

#[cfg(test)]
mod live_tests {
    use super::*;
    use crate::fixtures::{
        write_basic_fixture, write_edge_case_fixture, write_multi_row_group_fixture,
    };
    use crate::scalar::Scalar;
    use tempfile::tempdir;

    /// Writes a real fixture with `ArrowWriter`, reads it back with this
    /// crate's own footer-only inspection logic, and checks every claim
    /// against values independently known from how the fixture was
    /// constructed - schema, row/row-group counts, and per-column
    /// min/max/null-count all have to match what was actually written,
    /// not just "look plausible."
    #[test]
    fn inspects_a_real_single_row_group_parquet_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("basic.parquet");
        let fixture = write_basic_fixture(&path);

        let report = inspect(&path).expect("inspecting a real parquet file");

        assert_eq!(report.num_rows, fixture.ids.len() as i64);
        assert_eq!(report.num_row_groups, 1);
        assert_eq!(report.row_groups.len(), 1);
        assert_eq!(report.row_groups[0].num_rows, fixture.ids.len() as i64);

        let names: Vec<&str> = report.schema.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["id", "name", "score", "active"]);
        assert_eq!(report.schema[0].physical_type, "INT32");
        assert_eq!(report.schema[1].physical_type, "BYTE_ARRAY");
        // Arrow's own writer annotates a Utf8 column with the modern
        // `LogicalType::STRING` (Debug-formatted as "String" here),
        // which `parquet-cli`'s own schema dump also shows in preference
        // to the legacy `ConvertedType::UTF8` this tool falls back to
        // only when no modern logical type is present.
        assert_eq!(report.schema[1].logical_type.as_deref(), Some("String"));
        assert_eq!(report.schema[2].physical_type, "DOUBLE");
        assert_eq!(report.schema[3].physical_type, "BOOLEAN");

        assert_eq!(report.columns.len(), 4);

        let id_stats = &report.columns[0];
        assert_eq!(id_stats.null_count, Some(0));
        assert_eq!(
            id_stats.min,
            Some(Scalar::I32(*fixture.ids.iter().min().unwrap()))
        );
        assert_eq!(
            id_stats.max,
            Some(Scalar::I32(*fixture.ids.iter().max().unwrap()))
        );

        let name_stats = &report.columns[1];
        let expected_nulls = fixture.names.iter().filter(|n| n.is_none()).count() as u64;
        assert_eq!(name_stats.null_count, Some(expected_nulls));
        let mut present_names: Vec<&str> = fixture.names.iter().flatten().copied().collect();
        present_names.sort();
        assert_eq!(
            name_stats.min,
            Some(Scalar::Str(present_names.first().unwrap().to_string()))
        );
        assert_eq!(
            name_stats.max,
            Some(Scalar::Str(present_names.last().unwrap().to_string()))
        );

        let score_stats = &report.columns[2];
        assert_eq!(score_stats.null_count, Some(0));
        let min_score = fixture.scores.iter().cloned().fold(f64::INFINITY, f64::min);
        let max_score = fixture
            .scores
            .iter()
            .cloned()
            .fold(f64::NEG_INFINITY, f64::max);
        assert_eq!(score_stats.min, Some(Scalar::F64(min_score)));
        assert_eq!(score_stats.max, Some(Scalar::F64(max_score)));

        let active_stats = &report.columns[3];
        assert_eq!(active_stats.null_count, Some(0));
        assert_eq!(active_stats.min, Some(Scalar::Bool(false)));
        assert_eq!(active_stats.max, Some(Scalar::Bool(true)));
    }

    /// The multi-row-group fixture forces 3 real row groups (2, 2, 1
    /// rows). This confirms row-group-level reporting reflects that
    /// real split, and that per-column min/max/null-count are correctly
    /// merged *across* those row groups rather than only reflecting the
    /// first one.
    #[test]
    fn aggregates_stats_correctly_across_multiple_real_row_groups() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("multi.parquet");
        let fixture = write_multi_row_group_fixture(&path);

        let report = inspect(&path).expect("inspecting a real multi-row-group parquet file");

        assert_eq!(report.num_rows, fixture.codes.len() as i64);
        assert_eq!(report.num_row_groups, fixture.row_group_sizes.len());

        let actual_sizes: Vec<i64> = report.row_groups.iter().map(|rg| rg.num_rows).collect();
        assert_eq!(actual_sizes, fixture.row_group_sizes);

        let code_stats = &report.columns[0];
        let expected_nulls = fixture.codes.iter().filter(|c| c.is_none()).count() as u64;
        assert_eq!(code_stats.null_count, Some(expected_nulls));

        let present: Vec<i32> = fixture.codes.iter().flatten().copied().collect();
        assert_eq!(
            code_stats.min,
            Some(Scalar::I32(*present.iter().min().unwrap()))
        );
        assert_eq!(
            code_stats.max,
            Some(Scalar::I32(*present.iter().max().unwrap()))
        );
        // Sanity: the numeric min/max here (10 and 50) would come out
        // backwards under lexical-string comparison ("10" < "50" is
        // true either way, so pick values where it would actually
        // differ) - see scalar.rs's own dedicated regression test for
        // the case where lexical and numeric order disagree.
    }

    /// Opening something that isn't a Parquet file at all must fail
    /// cleanly with an error, not panic.
    #[test]
    fn inspecting_a_non_parquet_file_is_a_clean_error() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("not-parquet.txt");
        std::fs::write(&path, b"this is definitely not a parquet file").unwrap();

        let result = inspect(&path);
        assert!(result.is_err());
    }

    /// Opening a path that doesn't exist at all must also fail cleanly.
    #[test]
    fn inspecting_a_missing_file_is_a_clean_error() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("does-not-exist.parquet");
        let result = inspect(&path);
        assert!(result.is_err());
    }

    /// A column that is entirely null still has real statistics recorded
    /// (Arrow's writer always writes a null count), but there is no
    /// non-null value to report as a min or max - this must come out as
    /// `None`, not a bogus default like an empty string or zero.
    #[test]
    fn all_null_column_has_null_count_but_no_min_or_max() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("edge.parquet");
        write_edge_case_fixture(&path);

        let report = inspect(&path).expect("inspecting a real parquet file");
        let all_null_stats = &report.columns[0];
        assert_eq!(all_null_stats.null_count, Some(3));
        assert_eq!(all_null_stats.min, None);
        assert_eq!(all_null_stats.max, None);
    }

    /// A plain `Int64` column needs no logical/converted type to
    /// disambiguate its physical storage, and Arrow's writer correctly
    /// doesn't attach one - confirming the "no logical type" branch
    /// reports `None` rather than inventing a value.
    #[test]
    fn column_with_no_logical_type_reports_none_not_a_placeholder() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("edge.parquet");
        write_edge_case_fixture(&path);

        let report = inspect(&path).expect("inspecting a real parquet file");
        let plain_col = &report.schema[1];
        assert_eq!(plain_col.name, "plain");
        assert_eq!(plain_col.physical_type, "INT64");
        assert_eq!(plain_col.logical_type, None);

        let plain_stats = &report.columns[1];
        assert_eq!(plain_stats.null_count, Some(0));
        assert_eq!(plain_stats.min, Some(Scalar::I64(100)));
        assert_eq!(plain_stats.max, Some(Scalar::I64(300)));
    }

    /// Locates the real `parquetpeek` binary built alongside this test
    /// binary. `CARGO_BIN_EXE_*` (Cargo's usual answer to this) is only
    /// defined for tests under `tests/`, not for `#[cfg(test)]` unit
    /// tests inside `src/` - so this walks up from the running test
    /// binary's own path (`target/debug/deps/parquetpeek-<hash>`) to its
    /// sibling `target/debug/parquetpeek`, the same binary
    /// `cargo build --all-targets` (which this crate's CI runs before
    /// `cargo test --lib`) always produces there.
    fn binary_path() -> std::path::PathBuf {
        let mut path = std::env::current_exe().expect("path of the running test binary");
        path.pop(); // drop the test binary's own filename
        if path.ends_with("deps") {
            path.pop();
        }
        path.push("parquetpeek");
        path
    }

    /// End-to-end through the actual built CLI binary, not just the
    /// library: writes a real fixture, runs `parquetpeek` as a real
    /// subprocess against it, and checks the real stdout contains the
    /// facts a human running this tool would need to see. Skips (rather
    /// than fails) if the binary hasn't been built yet - `cargo test
    /// --lib` on its own does not build the `[[bin]]` target, only
    /// `cargo build --all-targets` or a plain `cargo test` does; this
    /// crate's own CI always runs the former first, same as the sibling
    /// `pgqueue` crate elsewhere in this monorepo skips its own
    /// live-database tests rather than failing when the precondition
    /// isn't met.
    #[test]
    fn cli_binary_reports_real_fixture_contents() {
        let bin = binary_path();
        if !bin.exists() {
            eprintln!(
                "skipping cli_binary_reports_real_fixture_contents: {} not built \
                 (run `cargo build --all-targets` first)",
                bin.display()
            );
            return;
        }

        let dir = tempdir().unwrap();
        let path = dir.path().join("cli.parquet");
        write_basic_fixture(&path);

        let output = std::process::Command::new(&bin)
            .arg(&path)
            .output()
            .expect("running the real parquetpeek binary");

        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains("5 row(s), 1 row group(s)"));
        assert!(stdout.contains("id"));
        assert!(stdout.contains("INT32"));
        assert!(stdout.contains("name"));
        assert!(stdout.contains("nulls=1"));
    }

    /// The CLI must fail with a nonzero exit code (not panic) against a
    /// file that doesn't exist. Skips under the same precondition as
    /// [`cli_binary_reports_real_fixture_contents`].
    #[test]
    fn cli_binary_exits_nonzero_on_missing_file() {
        let bin = binary_path();
        if !bin.exists() {
            eprintln!(
                "skipping cli_binary_exits_nonzero_on_missing_file: {} not built \
                 (run `cargo build --all-targets` first)",
                bin.display()
            );
            return;
        }

        let output = std::process::Command::new(&bin)
            .arg("/no/such/file.parquet")
            .output()
            .expect("running the real parquetpeek binary");

        assert!(!output.status.success());
    }
}
