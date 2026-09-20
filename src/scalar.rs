use std::cmp::Ordering;
use std::fmt;

use parquet::file::statistics::Statistics;

/// A decoded min/max value from a Parquet column-chunk statistics
/// struct, typed enough to compare correctly (a numeric column's min/max
/// must be compared numerically, not as text - `"9" > "10"` lexically
/// but not numerically).
#[derive(Debug, Clone, PartialEq)]
pub enum Scalar {
    Bool(bool),
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    Str(String),
    Bytes(Vec<u8>),
}

impl fmt::Display for Scalar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Scalar::Bool(v) => write!(f, "{v}"),
            Scalar::I32(v) => write!(f, "{v}"),
            Scalar::I64(v) => write!(f, "{v}"),
            Scalar::F32(v) => write!(f, "{v}"),
            Scalar::F64(v) => write!(f, "{v}"),
            Scalar::Str(v) => write!(f, "{v}"),
            Scalar::Bytes(v) => write!(f, "0x{}", hex_encode(v)),
        }
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Compares two scalars of the (assumed) same variant. Encountering
/// mismatched variants would mean the same column reported two
/// different physical types across row groups, which real Cargo/Parquet
/// never produces for one column - it's treated as "equal" defensively
/// rather than panicking.
fn compare(a: &Scalar, b: &Scalar) -> Ordering {
    match (a, b) {
        (Scalar::Bool(x), Scalar::Bool(y)) => x.cmp(y),
        (Scalar::I32(x), Scalar::I32(y)) => x.cmp(y),
        (Scalar::I64(x), Scalar::I64(y)) => x.cmp(y),
        (Scalar::F32(x), Scalar::F32(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
        (Scalar::F64(x), Scalar::F64(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
        (Scalar::Str(x), Scalar::Str(y)) => x.cmp(y),
        (Scalar::Bytes(x), Scalar::Bytes(y)) => x.cmp(y),
        _ => Ordering::Equal,
    }
}

/// Folds `b` into the running minimum `a`, taking ownership of whichever
/// is smaller - used to merge one column's statistics across multiple
/// row groups.
pub fn scalar_min(a: Scalar, b: Scalar) -> Scalar {
    match compare(&a, &b) {
        Ordering::Greater => b,
        _ => a,
    }
}

/// Folds `b` into the running maximum `a`. See [`scalar_min`].
pub fn scalar_max(a: Scalar, b: Scalar) -> Scalar {
    match compare(&a, &b) {
        Ordering::Less => b,
        _ => a,
    }
}

/// Decodes a `ByteArray`'s bytes as UTF-8 if possible (the common case -
/// a string column), falling back to raw bytes for genuine binary data.
fn byte_array_scalar(bytes: &[u8]) -> Scalar {
    match std::str::from_utf8(bytes) {
        Ok(text) => Scalar::Str(text.to_string()),
        Err(_) => Scalar::Bytes(bytes.to_vec()),
    }
}

/// Extracts the typed minimum from a column chunk's statistics, if
/// present. `Int96` (the legacy 12-byte timestamp encoding) has no
/// stable numeric interpretation without also knowing the column's
/// logical type, so it's deliberately left undecoded here - see the
/// README's scope-limits section.
pub fn min_from_statistics(stats: &Statistics) -> Option<Scalar> {
    match stats {
        Statistics::Boolean(s) => s.min_opt().map(|v| Scalar::Bool(*v)),
        Statistics::Int32(s) => s.min_opt().map(|v| Scalar::I32(*v)),
        Statistics::Int64(s) => s.min_opt().map(|v| Scalar::I64(*v)),
        Statistics::Float(s) => s.min_opt().map(|v| Scalar::F32(*v)),
        Statistics::Double(s) => s.min_opt().map(|v| Scalar::F64(*v)),
        Statistics::ByteArray(s) => s.min_opt().map(|v| byte_array_scalar(v.data())),
        Statistics::FixedLenByteArray(s) => s.min_opt().map(|v| byte_array_scalar(v.data())),
        Statistics::Int96(_) => None,
    }
}

/// Extracts the typed maximum from a column chunk's statistics. See
/// [`min_from_statistics`].
pub fn max_from_statistics(stats: &Statistics) -> Option<Scalar> {
    match stats {
        Statistics::Boolean(s) => s.max_opt().map(|v| Scalar::Bool(*v)),
        Statistics::Int32(s) => s.max_opt().map(|v| Scalar::I32(*v)),
        Statistics::Int64(s) => s.max_opt().map(|v| Scalar::I64(*v)),
        Statistics::Float(s) => s.max_opt().map(|v| Scalar::F32(*v)),
        Statistics::Double(s) => s.max_opt().map(|v| Scalar::F64(*v)),
        Statistics::ByteArray(s) => s.max_opt().map(|v| byte_array_scalar(v.data())),
        Statistics::FixedLenByteArray(s) => s.max_opt().map(|v| byte_array_scalar(v.data())),
        Statistics::Int96(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_min_is_numeric_not_lexical() {
        // Lexically "10" < "9", numerically 9 < 10 - this locks in that
        // scalar_min compares as the typed value, not as text.
        let a = Scalar::I32(10);
        let b = Scalar::I32(9);
        assert_eq!(scalar_min(a, b), Scalar::I32(9));
    }

    #[test]
    fn numeric_max_is_numeric_not_lexical() {
        let a = Scalar::I32(10);
        let b = Scalar::I32(9);
        assert_eq!(scalar_max(a, b), Scalar::I32(10));
    }

    #[test]
    fn float_min_max_use_partial_ord() {
        assert_eq!(
            scalar_min(Scalar::F64(1.5), Scalar::F64(-2.5)),
            Scalar::F64(-2.5)
        );
        assert_eq!(
            scalar_max(Scalar::F64(1.5), Scalar::F64(-2.5)),
            Scalar::F64(1.5)
        );
    }

    #[test]
    fn string_min_max_are_lexical() {
        assert_eq!(
            scalar_min(Scalar::Str("bob".into()), Scalar::Str("alice".into())),
            Scalar::Str("alice".into())
        );
        assert_eq!(
            scalar_max(Scalar::Str("bob".into()), Scalar::Str("alice".into())),
            Scalar::Str("bob".into())
        );
    }

    #[test]
    fn display_formats_each_variant_plainly() {
        assert_eq!(Scalar::Bool(true).to_string(), "true");
        assert_eq!(Scalar::I64(-7).to_string(), "-7");
        assert_eq!(Scalar::Str("hi".into()).to_string(), "hi");
        assert_eq!(Scalar::Bytes(vec![0xde, 0xad]).to_string(), "0xdead");
    }
}
