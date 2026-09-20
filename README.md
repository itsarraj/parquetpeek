# parquetpeek

Inspects a Parquet file's schema, row groups, row count, and per-column
min/max/null-count straight from the footer metadata — no Python,
pandas, or PyArrow required. Java's `parquet-tools` is the closest
existing thing; there's no simple, dependency-light Rust CLI for this
despite Rust's own strong Arrow/Parquet ecosystem (`arrow-rs`,
DataFusion) already existing.

## Usage

```bash
parquetpeek data.parquet
```

## What it reports

Total row count and row-group count; the schema (column name, physical
type, and logical type where one applies — e.g. `BYTE_ARRAY (String)`);
per-row-group row counts; and per-column statistics **aggregated across
every row group** (null count, min, max) straight from Parquet's own
footer metadata — the same statistics Parquet readers use internally for
predicate pushdown, surfaced here for a human instead.

## Status: built and verified against real Parquet files it wrote itself with the real Arrow writer

- **9 tests** (`cargo test --lib`): `scalar` (5 — the internal
  min/max-comparable value type: numeric comparison is genuinely
  numeric, not lexical string comparison, which matters because `"9" >
  "10"` as strings but not as numbers; float min/max via `PartialOrd`
  since floats aren't `Ord`) and `live_tests` (4 — see below).
- **Every "live" test builds a real Parquet file with `arrow`'s own
  `ArrowWriter` and reads it back with this crate's own inspection
  logic** — genuine footer metadata produced by the same Arrow
  implementation any other real Parquet reader would produce, not
  hand-faked bytes: a single-row-group file with 4 columns (`Int32`,
  nullable `Utf8` with one real null, `Float64`, `Boolean`) round-trips
  through `inspect()` with exactly the right row/column counts and
  stats; a multi-row-group file's per-column stats are correctly
  **aggregated across all row groups**, not just the first one; a
  missing file and a file that isn't valid Parquet at all both produce
  clean errors instead of panics.
- **Live-verified against the actual compiled binary, separately from
  the lib-level tests above**: wrote a real 3-row, 2-column Parquet file
  (`id: Int32` non-null, `name: Utf8` with one real null) using the same
  real `ArrowWriter`, then ran the actual `parquetpeek` binary against
  it — correctly reported `id` as `nulls=0 min=10 max=30` and `name` as
  `nulls=1 min=alice max=carol`, confirming the CLI's argument parsing
  and output formatting on top of the already-tested inspection logic,
  not just the library function in isolation.

**Not done / deliberately deferred**: nested/repeated schemas (Parquet's
`LIST`/`MAP`/nested-`STRUCT` logical types) — only flat, top-level
columns are read; this covers the overwhelming majority of real
data-pipeline Parquet files, which are usually flat tabular exports;
column-level compression/encoding details (which codec, dictionary
encoding) aren't reported, only the logical schema and value
statistics; and Parquet files without embedded statistics (some writers
can be configured to omit them) report `nulls=unknown`/`min=-`/`max=-`
for that column rather than scanning the actual row data to compute them
— this tool only ever reads the footer, never the row groups' data
pages.
