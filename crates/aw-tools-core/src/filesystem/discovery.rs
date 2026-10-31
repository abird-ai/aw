//! Canonical discovery engine over an authorized OS filesystem root.
//! Both AW and future dotlink adapters reuse traversal, selection and
//! rendering instead of maintaining their own copies.
use super::{
    backend::MetadataBackend, walk, Entry, Filter, MetaKey, Progress, Selection, Sort,
    MAX_WALK_DEPTH,
};
pub struct Options<'a> {
    pub path: &'a str,
    pub depth: usize,
    pub hidden: bool,
    pub gitignore: bool,
    pub filter: &'a Filter,
    pub sort: Sort,
    pub meta: &'a [MetaKey],
    pub offset: usize,
    pub limit: usize,
    pub basenames: bool,
    pub output_bytes: usize,
}
pub struct Page {
    pub content: String,
    pub truncated: bool,
    pub progress: Progress,
}
#[derive(Debug)]
pub enum Error<E> {
    InvalidOptions(&'static str),
    Walk(walk::WalkError<E>),
}
pub fn run<B: MetadataBackend>(
    backend: &B,
    options: Options<'_>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Page, Error<B::Error>> {
    if options.depth == 0 || options.depth > MAX_WALK_DEPTH {
        return Err(Error::InvalidOptions("invalid bounded discovery depth"));
    }
    if options.meta.len() > 5 {
        return Err(Error::InvalidOptions("too many metadata columns"));
    }
    let mut selection = Selection::new(options.sort, options.offset, options.limit)
        .map_err(Error::InvalidOptions)?;
    let mut progress = Progress::default();
    let walk_state = walk::walk(
        backend,
        options.path,
        options.depth,
        options.hidden,
        options.gitignore,
        cancelled,
        |node| {
            if !options.filter.kind.accepts(node.kind) {
                return Ok(false);
            }
            let meta = match backend.entry_meta(&node.path) {
                Ok(meta) => meta,
                Err(_) => {
                    progress.unreadable_entries = true;
                    return Ok(false);
                }
            };
            let relative = if options.path.is_empty() {
                node.path.as_str()
            } else {
                node.path
                    .strip_prefix(options.path)
                    .and_then(|path| path.strip_prefix('/'))
                    .unwrap_or(&node.path)
            };
            let entry = Entry {
                path: node.path.clone(),
                name: node.name.clone(),
                meta,
            };
            if options.filter.matches(&entry, relative) {
                selection.add(entry);
            }
            Ok(false)
        },
    )
    .map_err(Error::Walk)?;
    progress.directory_budget |= walk_state.directory_budget;
    progress.traversal_error |= walk_state.traversal_error;
    progress.ignore_rules |= walk_state.ignore_rules;
    progress.path_encoding |= walk_state.path_encoding;
    let (entries, more) = selection.finish();
    if let Some(first) = entries.first() {
        let displayed = if options.basenames {
            &first.name
        } else {
            &first.path
        };
        // The renderer reserves control/footer space. If even the first
        // semantic unit cannot fit, a same-offset more marker would loop.
        let ceiling = options.output_bytes.min(super::MAX_RENDER_BYTES);
        let reserved = 128usize.min(ceiling);
        let needed = super::render_line(first, displayed, options.meta).len();
        if needed.saturating_add(reserved) > ceiling {
            return Err(Error::InvalidOptions(
                "first discovery entry exceeds result ceiling; narrow query",
            ));
        }
    }
    let (content, truncated) = super::render(
        &entries,
        |entry| {
            if options.basenames {
                &entry.name
            } else {
                &entry.path
            }
        },
        options.meta,
        options.offset,
        more,
        progress,
        options.output_bytes,
    );
    Ok(Page {
        content,
        truncated,
        progress,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::filesystem::{EntryMeta, Kind};
    use std::collections::BTreeMap;
    struct Fake {
        dirs: BTreeMap<String, Vec<walk::WalkEntry>>,
        meta: BTreeMap<String, EntryMeta>,
    }
    impl walk::Backend for Fake {
        type Error = &'static str;
        fn list(&self, path: &str, _: usize) -> Result<walk::Listing, Self::Error> {
            Ok(walk::Listing {
                incomplete: false,
                path_encoding: false,
                entries: self.dirs.get(path).cloned().ok_or("absent")?,
            })
        }
        fn read_bytes(&self, _: &str, _: usize) -> Result<Vec<u8>, Self::Error> {
            Err("absent")
        }
        fn classify(&self, _: &&'static str) -> walk::BackendErrorKind {
            walk::BackendErrorKind::Missing
        }
    }
    impl MetadataBackend for Fake {
        fn entry_meta(&self, path: &str) -> Result<EntryMeta, Self::Error> {
            self.meta.get(path).cloned().ok_or("absent")
        }
    }
    #[test]
    fn oversized_first_record_is_actionable_error_not_stuck_cursor() {
        let name = format!("{}.rs", "a".repeat(600));
        let fake = Fake {
            dirs: BTreeMap::from([(
                "".into(),
                vec![walk::WalkEntry {
                    name: name.clone(),
                    path: name.clone(),
                    kind: Kind::File,
                    hidden: false,
                }],
            )]),
            meta: BTreeMap::from([(
                name.clone(),
                EntryMeta {
                    kind: Kind::File,
                    ..Default::default()
                },
            )]),
        };
        let filter = Filter::compile(None, None, Kind::File, None, None, None).unwrap();
        let result = run(
            &fake,
            Options {
                path: "",
                depth: 1,
                hidden: false,
                gitignore: false,
                filter: &filter,
                sort: Sort::Path,
                meta: &[],
                offset: 0,
                limit: 1,
                basenames: true,
                output_bytes: 400,
            },
            &|| false,
        );
        assert!(matches!(result, Err(Error::InvalidOptions(_))));
    }
    #[test]
    fn shared_discovery_preserves_global_page_order() {
        let names = ["c.rs", "a.rs", "b.rs"];
        let fake = Fake {
            dirs: BTreeMap::from([(
                "".into(),
                names
                    .iter()
                    .map(|n| walk::WalkEntry {
                        name: (*n).into(),
                        path: (*n).into(),
                        kind: Kind::File,
                        hidden: false,
                    })
                    .collect(),
            )]),
            meta: names
                .iter()
                .map(|n| {
                    (
                        (*n).into(),
                        EntryMeta {
                            kind: Kind::File,
                            ..Default::default()
                        },
                    )
                })
                .collect(),
        };
        let filter = Filter::compile(Some("*.rs"), None, Kind::File, None, None, None).unwrap();
        let opts = Options {
            path: "",
            depth: 1,
            hidden: false,
            gitignore: false,
            filter: &filter,
            sort: Sort::Path,
            meta: &[],
            offset: 1,
            limit: 1,
            basenames: false,
            output_bytes: 4096,
        };
        let page = run(&fake, opts, &|| false).unwrap();
        assert_eq!(page.content, "f  b.rs\nmore offset=2\n");
        assert!(page.truncated);
    }
}
