use std::path::PathBuf;

use clap::Parser;

use parquetpeek::inspect;

#[derive(Parser)]
#[command(
    name = "parquetpeek",
    about = "Inspects a Parquet file's schema, row groups, and per-column min/max/null-count from its footer metadata"
)]
struct Cli {
    /// Path to a .parquet file.
    file: PathBuf,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let report = inspect(&cli.file)?;

    println!("{}", cli.file.display());
    println!(
        "{} row(s), {} row group(s)",
        report.num_rows, report.num_row_groups
    );
    println!();

    println!("schema:");
    for col in &report.schema {
        match &col.logical_type {
            Some(logical) => println!("  {:<20} {} ({})", col.name, col.physical_type, logical),
            None => println!("  {:<20} {}", col.name, col.physical_type),
        }
    }
    println!();

    println!("row groups:");
    for rg in &report.row_groups {
        println!("  [{}] {} row(s)", rg.index, rg.num_rows);
    }
    println!();

    println!("column stats (aggregated across all row groups):");
    for col in &report.columns {
        let nulls = col
            .null_count
            .map(|n| n.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let min = col
            .min
            .as_ref()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "-".to_string());
        let max = col
            .max
            .as_ref()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "-".to_string());
        println!(
            "  {:<20} nulls={:<10} min={:<15} max={}",
            col.name, nulls, min, max
        );
    }

    Ok(())
}
