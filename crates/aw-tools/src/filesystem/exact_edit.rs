//! Independent exact replacements in one original UTF-8 snapshot.
//! All matches are selected before one staged publication; newly created text
//! cannot be a later search anchor. No transport, IO or model grant here.
use memchr::memmem;
use std::ops::Range;

#[derive(Debug, Clone, Copy)]
pub struct Replacement<'a> {
    pub old: &'a str,
    pub new: &'a str,
    pub all: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditError {
    Empty,
    Missing(usize),
    Ambiguous { edit: usize, matches: usize },
    Overlap { first: usize, second: usize },
    TooMany,
    OutputLimit,
}
#[derive(Debug)]
struct Span<'a> {
    range: Range<usize>,
    replacement: &'a str,
    edit: usize,
}
pub const MAX_EXACT_REPLACEMENTS: usize = 512;

/// All edit indices are 1-based, stable model coordinates.
pub fn apply(
    source: &str,
    edits: &[Replacement<'_>],
    max_bytes: usize,
) -> Result<String, EditError> {
    if edits.is_empty() || edits.len() > MAX_EXACT_REPLACEMENTS {
        return Err(EditError::TooMany);
    }
    let mut spans = Vec::<Span<'_>>::new();
    for (index, edit) in edits.iter().enumerate() {
        let id = index + 1;
        if edit.old.is_empty() {
            return Err(EditError::Empty);
        }
        let mut matches = 0usize;
        for start in memmem::find_iter(source.as_bytes(), edit.old.as_bytes()) {
            matches += 1;
            if (edit.all || matches == 1) && edit.old != edit.new {
                // Identity substitutions validate their matches but never
                // own a changed source interval. A real edit may therefore
                // overlap a separate no-op without a false conflict.
                spans.push(Span {
                    range: start..start + edit.old.len(),
                    replacement: edit.new,
                    edit: id,
                });
            }
        }
        if matches == 0 {
            return Err(EditError::Missing(id));
        }
        if !edit.all && matches != 1 {
            return Err(EditError::Ambiguous { edit: id, matches });
        }
    }
    spans.sort_unstable_by(|a, b| {
        a.range
            .start
            .cmp(&b.range.start)
            .then_with(|| a.range.end.cmp(&b.range.end))
            .then_with(|| a.edit.cmp(&b.edit))
    });
    for window in spans.windows(2) {
        if window[1].range.start < window[0].range.end {
            return Err(EditError::Overlap {
                first: window[0].edit,
                second: window[1].edit,
            });
        }
    }
    // Selected spans are non-overlapping. Enforce the FINAL resulting
    // length, not an intermediate source-order length: an early expansion
    // may be offset by a later independent contraction.
    let mut removed = 0usize;
    let mut added = 0usize;
    for span in &spans {
        removed = removed
            .checked_add(span.range.len())
            .ok_or(EditError::OutputLimit)?;
        added = added
            .checked_add(span.replacement.len())
            .ok_or(EditError::OutputLimit)?;
    }
    let output_len = source
        .len()
        .checked_sub(removed)
        .and_then(|n| n.checked_add(added))
        .ok_or(EditError::OutputLimit)?;
    if output_len > max_bytes {
        return Err(EditError::OutputLimit);
    }
    let mut output = String::with_capacity(output_len);
    let mut prev = 0;
    for span in spans {
        // UTF-8 needle boundaries always align to UTF-8 source boundaries.
        output.push_str(&source[prev..span.range.start]);
        output.push_str(span.replacement);
        prev = span.range.end;
    }
    output.push_str(&source[prev..]);
    debug_assert_eq!(output.len(), output_len);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_original_snapshot_even_when_second_needle_created() {
        let result = apply(
            "alpha beta",
            &[
                Replacement {
                    old: "alpha",
                    new: "beta",
                    all: false,
                },
                Replacement {
                    old: "beta",
                    new: "gamma",
                    all: false,
                },
            ],
            1024,
        )
        .unwrap();
        assert_eq!(result, "beta gamma");
    }
    #[test]
    fn overlaps_and_duplicates_are_rejected() {
        let err = apply(
            "foobarbaz",
            &[
                Replacement {
                    old: "foobar",
                    new: "X",
                    all: false,
                },
                Replacement {
                    old: "bar",
                    new: "Y",
                    all: false,
                },
            ],
            1024,
        );
        assert!(matches!(
            err,
            Err(EditError::Overlap {
                first: 1,
                second: 2
            })
        ));
        let err = apply(
            "abc",
            &[
                Replacement {
                    old: "abc",
                    new: "ABC",
                    all: false,
                },
                Replacement {
                    old: "abc",
                    new: "Abc",
                    all: false,
                },
            ],
            1024,
        );
        assert!(matches!(
            err,
            Err(EditError::Overlap {
                first: 1,
                second: 2
            })
        ));
    }
    #[test]
    fn validated_identity_edit_does_not_block_independent_changed_spans() {
        let result = apply(
            "abc",
            &[
                Replacement {
                    old: "ab",
                    new: "ab",
                    all: false,
                },
                Replacement {
                    old: "b",
                    new: "B",
                    all: false,
                },
            ],
            3,
        )
        .unwrap();
        assert_eq!(result, "aBc");
        let no_op = apply(
            "abcabc",
            &[Replacement {
                old: "abc",
                new: "abc",
                all: true,
            }],
            6,
        )
        .unwrap();
        assert_eq!(no_op, "abcabc");
        let mixed = apply(
            "x y x",
            &[
                Replacement {
                    old: "x",
                    new: "x",
                    all: true,
                },
                Replacement {
                    old: "y",
                    new: "Y",
                    all: false,
                },
            ],
            5,
        )
        .unwrap();
        assert_eq!(mixed, "x Y x");
        assert!(matches!(
            apply(
                "xx",
                &[Replacement {
                    old: "x",
                    new: "x",
                    all: false
                }],
                2
            ),
            Err(EditError::Ambiguous { matches: 2, .. })
        ));
    }
    #[test]
    fn ambiguous_missing_and_unicode_replace_all() {
        assert!(matches!(
            apply(
                "aaa",
                &[Replacement {
                    old: "a",
                    new: "b",
                    all: false
                }],
                1024
            ),
            Err(EditError::Ambiguous {
                edit: 1,
                matches: 3
            })
        ));
        assert!(matches!(
            apply(
                "abc",
                &[Replacement {
                    old: "missing",
                    new: "x",
                    all: true
                }],
                1024
            ),
            Err(EditError::Missing(1))
        ));
        assert_eq!(
            apply(
                "🙂é🙂é",
                &[
                    Replacement {
                        old: "🙂",
                        new: "x",
                        all: true
                    },
                    Replacement {
                        old: "é",
                        new: "y",
                        all: true
                    },
                ],
                1024
            )
            .unwrap(),
            "xyxy"
        );
    }
    #[test]
    fn no_op_and_growth_are_accounted_before_allocation() {
        assert_eq!(
            apply(
                "abc",
                &[Replacement {
                    old: "b",
                    new: "b",
                    all: false
                }],
                3
            )
            .unwrap(),
            "abc"
        );
        assert!(matches!(
            apply(
                "abc",
                &[Replacement {
                    old: "b",
                    new: "verylong",
                    all: false
                }],
                5
            ),
            Err(EditError::OutputLimit)
        ));
    }
    #[test]
    fn independent_expansion_and_shrink_validate_final_size_not_midpoint() {
        let original = "a___bbb";
        let result = apply(
            original,
            &[
                Replacement {
                    old: "a",
                    new: "AAAA",
                    all: false,
                },
                Replacement {
                    old: "bbb",
                    new: "",
                    all: false,
                },
            ],
            original.len(),
        )
        .unwrap();
        assert_eq!(result, "AAAA___");
    }
    #[test]
    fn input_edit_order_does_not_change_disjoint_result() {
        let first = [
            Replacement {
                old: "x",
                new: "X",
                all: true,
            },
            Replacement {
                old: "y",
                new: "Y",
                all: true,
            },
        ];
        let reverse = [first[1], first[0]];
        assert_eq!(
            apply("x y xy", &first, 1024).unwrap(),
            apply("x y xy", &reverse, 1024).unwrap()
        );
    }
}
