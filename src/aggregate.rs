//! Streaming reductions over query rows. Memory is O(1) for [`aggregate`] and
//! O(K) for [`group_by`], where K is the number of groups.

use alloc::{collections::BTreeMap, vec::Vec};

use crate::{
    engine::{QueryError, Row},
    source::{FieldPath, Value},
};

/// A reduction over a stream of rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Metric {
    /// Number of rows.
    Count,
    /// Sum of numeric values. Integers stay integers unless a float appears.
    Sum(FieldPath),
    /// Smallest non-null value, by [`Value`] order.
    Min(FieldPath),
    /// Largest non-null value, by [`Value`] order.
    Max(FieldPath),
    /// Mean of numeric values, as a float.
    Avg(FieldPath),
}

impl Metric {
    fn path(&self) -> Option<&FieldPath> {
        match self {
            Metric::Count => None,
            Metric::Sum(p) | Metric::Min(p) | Metric::Max(p) | Metric::Avg(p) => Some(p),
        }
    }
}

/// Running state for one group. Non-numeric values are ignored by `Sum` and `Avg`.
#[derive(Default)]
struct Acc {
    rows: u64,
    numeric: u64,
    int_sum: i64,
    float_sum: f64,
    int_overflow: bool,
    has_float: bool,
    min: Option<Value>,
    max: Option<Value>,
}

impl Acc {
    fn push(&mut self, metric: &Metric, value: Option<&Value>) {
        self.rows += 1;
        let Some(v) = value.filter(|v| !matches!(v, Value::Null)) else {
            return;
        };
        match metric {
            Metric::Count => {}
            Metric::Min(_) => {
                if self.min.as_ref().is_none_or(|m| v < m) {
                    self.min = Some(v.clone());
                }
            }
            Metric::Max(_) => {
                if self.max.as_ref().is_none_or(|m| v > m) {
                    self.max = Some(v.clone());
                }
            }
            Metric::Sum(_) | Metric::Avg(_) => match v {
                Value::Int(i) => {
                    self.numeric += 1;
                    self.float_sum += *i as f64;
                    match self.int_sum.checked_add(*i) {
                        Some(s) => self.int_sum = s,
                        None => self.int_overflow = true,
                    }
                }
                Value::Float(f) => {
                    self.numeric += 1;
                    self.float_sum += f;
                    self.has_float = true;
                }
                _ => {}
            },
        }
    }

    fn finish(&self, metric: &Metric) -> Value {
        match metric {
            Metric::Count => Value::Int(self.rows as i64),
            Metric::Sum(_) => {
                if self.has_float || self.int_overflow {
                    Value::Float(self.float_sum)
                } else {
                    Value::Int(self.int_sum)
                }
            }
            Metric::Avg(_) => {
                if self.numeric == 0 {
                    Value::Null
                } else {
                    Value::Float(self.float_sum / self.numeric as f64)
                }
            }
            Metric::Min(_) => self.min.clone().unwrap_or(Value::Null),
            Metric::Max(_) => self.max.clone().unwrap_or(Value::Null),
        }
    }
}

fn metric_value<'a>(metric: &Metric, row: &Row<'a>) -> Option<&'a Value> {
    metric.path().and_then(|p| row.entity.field(p))
}

/// Reduces all rows to one value. Errors from the stream stop the reduction.
/// `Count` of zero rows is `0`. `Sum` of zero numeric values is `0`. `Min`, `Max`,
/// and `Avg` of zero values are `Null`.
pub fn aggregate<'a, I>(rows: I, metric: &Metric) -> Result<Value, QueryError>
where
    I: IntoIterator<Item = Result<Row<'a>, QueryError>>,
{
    let mut acc = Acc::default();
    for row in rows {
        let row = row?;
        acc.push(metric, metric_value(metric, &row));
    }
    Ok(acc.finish(metric))
}

/// Groups rows by the value of `key` and reduces each group. Rows missing the key
/// fall into a `Null` group. Results are sorted by key.
pub fn group_by<'a, I>(
    rows: I,
    key: &FieldPath,
    metric: &Metric,
) -> Result<Vec<(Value, Value)>, QueryError>
where
    I: IntoIterator<Item = Result<Row<'a>, QueryError>>,
{
    let mut groups: BTreeMap<Value, Acc> = BTreeMap::new();
    for row in rows {
        let row = row?;
        let k = row.entity.field(key).cloned().unwrap_or(Value::Null);
        groups
            .entry(k)
            .or_default()
            .push(metric, metric_value(metric, &row));
    }
    Ok(groups
        .iter()
        .map(|(k, acc)| (k.clone(), acc.finish(metric)))
        .collect())
}
