//! One mutation's aggregate planning/receipt budgets shared by AW and dotlink.
//! Planning is pure and synchronous; the host owns filesystem authority,
//! execution/durability, staging, commit and no-replay settlement.

use anyhow::{anyhow, bail, Result};
use serde_json::{Map, Value};

pub use crate::MAX_MUTATION_PLAN_BYTES;
pub const MAX_EDIT_OPS: usize = 512;
pub const MAX_EDIT_FILES: usize = 64;

/// Bound untrusted arrays while Serde is decoding, before materializing an
/// unbounded Vec. AW additionally caps its upstream JSON request envelope.
pub fn bounded_array<'de, D, T, const MAX: usize>(
    deserializer: D,
    tool: &'static str,
    unit: &'static str,
) -> std::result::Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    use serde::de::{Error as _, SeqAccess, Visitor};
    struct Bounded<T, const MAX: usize>(std::marker::PhantomData<T>, &'static str, &'static str);
    impl<'de, T: serde::Deserialize<'de>, const MAX: usize> Visitor<'de> for Bounded<T, MAX> {
        type Value = Vec<T>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            write!(f, "at most {MAX} {}", self.2)
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<Vec<T>, A::Error> {
            if seq.size_hint().is_some_and(|n| n > MAX) {
                return Err(A::Error::custom(format!(
                    "{}: at most {MAX} {}",
                    self.1, self.2
                )));
            }
            let mut result = Vec::new();
            while let Some(item) = seq.next_element::<T>()? {
                if result.len() == MAX {
                    return Err(A::Error::custom(format!(
                        "{}: at most {MAX} operations",
                        self.1
                    )));
                }
                result.push(item);
            }
            Ok(result)
        }
    }
    deserializer.deserialize_seq(Bounded::<T, MAX>(std::marker::PhantomData, tool, unit))
}

#[derive(Default, Debug, Clone, Copy)]
pub struct PlanBudget {
    source: usize,
    work: usize,
}
impl PlanBudget {
    pub fn remaining(&self) -> usize {
        MAX_MUTATION_PLAN_BYTES.saturating_sub(self.source.saturating_add(self.work))
    }
    pub fn copy_read_limit(&self, per_file: usize) -> Result<usize> {
        let limit = (self.remaining() / 2).min(per_file);
        if limit == 0 {
            bail!("aggregate mutation plan exceeds {MAX_MUTATION_PLAN_BYTES} bytes");
        }
        Ok(limit)
    }
    pub fn add(&mut self, source: usize, work: usize) -> Result<()> {
        let src = self
            .source
            .checked_add(source)
            .ok_or_else(|| anyhow!("source size overflow"))?;
        let wrk = self
            .work
            .checked_add(work)
            .ok_or_else(|| anyhow!("work size overflow"))?;
        Self::ensure(src, wrk)?;
        self.source = src;
        self.work = wrk;
        Ok(())
    }
    pub fn replace_work(&mut self, before: usize, after: usize) -> Result<()> {
        let next = self
            .work
            .checked_sub(before)
            .and_then(|n| n.checked_add(after))
            .ok_or_else(|| anyhow!("work size overflow"))?;
        Self::ensure(self.source, next)?;
        self.work = next;
        Ok(())
    }
    fn ensure(src: usize, wrk: usize) -> Result<()> {
        if src
            .checked_add(wrk)
            .is_none_or(|used| used > MAX_MUTATION_PLAN_BYTES)
        {
            bail!("aggregate mutation plan exceeds {MAX_MUTATION_PLAN_BYTES} bytes");
        }
        Ok(())
    }
}
fn largest_fitting_prefix(len: usize, fit: impl Fn(usize) -> bool) -> usize {
    let (mut low, mut high) = (0, len);
    while low < high {
        let mid = low + (high - low).div_ceil(2);
        if fit(mid) {
            low = mid
        } else {
            high = mid - 1;
        }
    }
    low
}

/// Prevalidate receipt metadata BEFORE any mutation. Rendering a result is
/// always infallible after one or many per-file commits.
pub struct MutationReceipt {
    ceiling: usize,
    metadata: Map<String, Value>,
}
impl MutationReceipt {
    pub fn new(ceiling: usize, metadata: Map<String, Value>) -> Result<Self> {
        let receipt = Self { ceiling, metadata };
        // Reserve worst-case decimal counters before the first mutation.
        if receipt.render(&[], &[], usize::MAX, usize::MAX).len() > ceiling {
            bail!("mutation receipt metadata cannot fit {ceiling} bytes before writing");
        }
        Ok(receipt)
    }
    fn render(&self, files: &[Value], failures: &[Value], skipped: usize, errors: usize) -> String {
        let mut value = self.metadata.clone();
        value.insert("files".into(), Value::Array(files.to_vec()));
        if !failures.is_empty() || errors > 0 {
            value.insert("failures".into(), Value::Array(failures.to_vec()));
        }
        if skipped > 0 || errors > 0 {
            value.insert("truncated".into(), Value::Bool(true));
            if skipped > 0 {
                value.insert("omitted_files".into(), Value::from(skipped));
            }
            if errors > 0 {
                value.insert("omitted_failures".into(), Value::from(errors));
            }
        }
        Value::Object(value).to_string()
    }
    /// The number of committed files is known only after the last publish.
    /// The preflight reserve makes completion render infallible.
    pub fn finish_with_count(&self, files: &[Value], failures: &[Value]) -> String {
        let mut actual = Self {
            ceiling: self.ceiling,
            metadata: self.metadata.clone(),
        };
        actual
            .metadata
            .insert("changed_files".into(), Value::from(files.len()));
        if actual.render(&[], &[], 1, 1).len() > actual.ceiling {
            // A safer response omits the count rather than claiming the wrong
            // number of commits. The file receipts still remain exact.
            actual.metadata.remove("changed_files");
        }
        actual.finish(files, failures)
    }
    pub fn finish(&self, files: &[Value], failures: &[Value]) -> String {
        let full = self.render(files, failures, 0, 0);
        if full.len() <= self.ceiling {
            return full;
        }
        let errors = largest_fitting_prefix(failures.len(), |n| {
            self.render(&[], &failures[..n], files.len(), failures.len() - n)
                .len()
                <= self.ceiling
        });
        let kept = largest_fitting_prefix(files.len(), |n| {
            self.render(
                &files[..n],
                &failures[..errors],
                files.len() - n,
                failures.len() - errors,
            )
            .len()
                <= self.ceiling
        });
        let result = self.render(
            &files[..kept],
            &failures[..errors],
            files.len() - kept,
            failures.len() - errors,
        );
        if result.len() > self.ceiling {
            return self.render(&[], &[], files.len(), failures.len());
        }
        result
    }
}
pub fn compact_receipt(mut value: Value, ceiling: usize) -> Result<String> {
    let full = value.to_string();
    if full.len() <= ceiling {
        return Ok(full);
    }
    let Some(map) = value.as_object_mut() else {
        bail!("mutation receipt exceeds {ceiling} bytes before write");
    };
    if map.remove("path").is_some() {
        map.insert("path_omitted".into(), Value::Bool(true));
    }
    let compact = value.to_string();
    if compact.len() > ceiling {
        bail!("mutation receipt exceeds {ceiling} bytes before write");
    }
    Ok(compact)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn aggregate_counts_both_original_and_work_before_alloc() {
        let mut budget = PlanBudget::default();
        budget.add(1024, 1024).unwrap();
        assert!(budget.replace_work(1024, MAX_MUTATION_PLAN_BYTES).is_err());
        assert_eq!(budget.remaining(), MAX_MUTATION_PLAN_BYTES - 2048);
    }
    #[test]
    fn bounded_receipt_never_claims_missing_files_present() {
        let mut meta = Map::new();
        meta.insert("changed_files".into(), Value::from(64));
        let receipt = MutationReceipt::new(170, meta).unwrap();
        let files = (0..64)
            .map(|i| json!({"path":format!("very-long-folder-{i:03}/file.txt"),"edits":2}))
            .collect::<Vec<_>>();
        let page: Value = serde_json::from_str(&receipt.finish(&files, &[])).unwrap();
        assert_eq!(page["changed_files"], 64);
        assert_eq!(page["truncated"], true);
        assert!(page["omitted_files"].as_u64().unwrap() > 0);
    }
}

#[cfg(test)]
mod extra_receipt_boundary_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn changing_committed_count_cannot_overrun_preflighted_receipt_budget() {
        let files = (0..64)
            .map(|i| json!({"path":format!("long/src/path/{i:03}/file.rs"),"hunks":1}))
            .collect::<Vec<_>>();
        for ceiling in 64..190 {
            let mut meta = Map::new();
            meta.insert("changed_files".into(), Value::from(0));
            meta.insert("mode".into(), Value::from("strict"));
            if let Ok(receipt) = MutationReceipt::new(ceiling, meta) {
                let out = receipt.finish_with_count(&files, &[]);
                assert!(
                    out.len() <= ceiling,
                    "cap={ceiling} bytes={} {out}",
                    out.len()
                );
                let decoded: Value = serde_json::from_str(&out).unwrap();
                assert_eq!(
                    decoded["files"].as_array().unwrap().len()
                        + decoded["omitted_files"].as_u64().unwrap_or(0) as usize,
                    files.len()
                );
            }
        }
    }
}

/// Model-facing mutation successes deliberately omit repeated input paths
/// and payloads. Distinguish an actual no-op from a committed namespace edit.
pub fn compact_success(changed_files: usize, edits: Option<usize>) -> String {
    if changed_files == 0 {
        return "unchanged".into();
    }
    match edits {
        Some(edits) => format!("ok {edits} edits {changed_files} files"),
        None => "ok".into(),
    }
}
