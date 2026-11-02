//! A source gem's module that nothing the app runs names is left out of
//! the tree, out loud.
//!
//! Rails eager-loads every Zeitwerk-managed file in production, so such a
//! module is defined; but a module only an untaken branch names is never
//! included, prepended or called. mission_control-jobs' engine prepends
//! `ActiveJob::QueueAdapters::ResqueExt` only when the app's adapters
//! include `:resque` (`engine_config::dead_spans` decides that branch), and
//! the module's bodies reach `Resque`, which an app on solid_queue has
//! not got.
//!
//! A file is a candidate when it is a source gem's and every class it
//! defines is the constant its path names (or nested in it), the way a
//! Zeitwerk file is, and no class body calls a method on another
//! receiver: a reopen of somebody else's class, or a file that patches
//! something as it loads, is always kept. Reached means its constant's
//! last segment is written, outside a dead branch, in a source the app
//! keeps, or its file's name is (a `require` loads it); the reached grow
//! from the app's own sources to a fixpoint.

use crate::det::{HashMap, HashSet};
use std::path::Path;

use crate::app::App;
use crate::dialect::LibraryClass;
use crate::expr::ExprNode;
use crate::span::Span;

pub(super) fn drop(app: &mut App, dir: &Path) {
    // Not `sources::path_of`: the registry was drained into `app.sources` before this runs.
    let path_of = |id: crate::span::FileId| (id.0 as usize).checked_sub(1).and_then(|i| app.sources.get(i)).map(|f| f.path.clone());
    let mut by_file: HashMap<String, Vec<usize>> = HashMap::default();
    for (i, lc) in app.library_classes.iter().enumerate() {
        if let Some(path) = file_of(lc).and_then(path_of) {
            by_file.entry(path).or_default().push(i);
        }
    }
    // Each candidate's constant leaf, and its file stem: a `require` loads a file whose constant no one names (`pundit/railtie`).
    let mut candidates: HashMap<String, (String, String)> = HashMap::default();
    for (path, classes) in &by_file {
        let Some(expected) = expected_constant(path) else { continue };
        let owned = classes.iter().all(|&i| {
            let lc = &app.library_classes[i];
            let name = lc.name.0.as_str();
            (name == expected || name.starts_with(&format!("{expected}::")))
                && !lc.unknown_calls.iter().any(|c| matches!(&*c.node, ExprNode::Send { recv: Some(_), .. }))
        });
        if owned && classes.iter().any(|&i| app.library_classes[i].name.0.as_str() == expected) {
            let leaf = expected.rsplit("::").next().unwrap_or(&expected).to_string();
            let stem = Path::new(path).file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string();
            candidates.insert(path.clone(), (leaf, stem));
        }
    }
    if candidates.is_empty() {
        return;
    }
    let dead = super::engine_config::dead_spans();
    let mut live: Vec<String> = Vec::new();
    let mut pending: HashMap<String, String> = HashMap::default();
    for source in &app.sources {
        let mut text = source.text.clone().into_bytes();
        for (_, start, end) in dead.iter().filter(|(path, ..)| same_file(path, &source.path, dir)) {
            let end = (*end as usize).min(text.len());
            for b in &mut text[(*start as usize).min(end)..end] {
                if *b != b'\n' {
                    *b = b' ';
                }
            }
        }
        let text = String::from_utf8_lossy(&text).into_owned();
        if candidates.contains_key(&source.path) {
            pending.insert(source.path.clone(), text);
        } else {
            live.push(text);
        }
    }
    let mut unreached: HashSet<String> = candidates.keys().cloned().collect();
    loop {
        let reached: Vec<String> = unreached
            .iter()
            .filter(|path| {
                let (leaf, stem) = &candidates[*path];
                live.iter().any(|text| names(text, leaf) || names(text, stem))
            })
            .cloned()
            .collect();
        if reached.is_empty() {
            break;
        }
        for path in reached {
            unreached.remove(&path);
            live.extend(pending.remove(&path));
        }
    }
    let gone_files: HashSet<crate::span::FileId> = app
        .sources
        .iter()
        .enumerate()
        .filter(|(_, f)| unreached.contains(&f.path))
        .map(|(i, _)| crate::span::FileId(i as u32 + 1))
        .collect();
    let mut dropped: Vec<LibraryClass> = Vec::new();
    let classes = std::mem::take(&mut app.library_classes);
    for lc in classes {
        let gone = file_of(&lc).is_some_and(|f| gone_files.contains(&f));
        if gone { dropped.push(lc) } else { app.library_classes.push(lc) }
    }
    app.file_programs.retain(|program| !unreached.iter().any(|path| same_file(path, &program.source, dir)));
    let names: HashSet<String> = dropped.iter().map(|lc| lc.name.0.as_str().to_string()).collect();
    for program in &mut app.file_programs {
        forget(&mut program.items, &names);
    }
    for lc in dropped {
        let span = file_of(&lc).map(|file| first_span(&lc, file)).unwrap_or_else(Span::synthetic);
        super::prism::push(crate::lower::residue_diagnostic(
            "unreached_gem_modules",
            lc.name.0.as_str(),
            span,
            "unreached",
            format!(
                "`{}` is a source gem's module nothing the app runs names (only a branch boot does not take, if anything), so it is left out of the tree",
                lc.name.0.as_str()
            ),
        ));
    }
}

fn forget(items: &mut Vec<crate::app::ProgramItem>, names: &HashSet<String>) {
    use crate::app::ProgramItem;
    for item in items.iter_mut() {
        match item {
            ProgramItem::Declared(ids) => ids.retain(|id| !names.contains(id.0.as_str())),
            ProgramItem::Branch { then_items, else_items, .. } => {
                forget(then_items, names);
                forget(else_items, names);
            }
            _ => {}
        }
    }
    items.retain(|item| !matches!(item, ProgramItem::Declared(ids) if ids.is_empty()));
}

/// `ActiveJob::QueueAdapters::ResqueExt` for `<mount>/<gem>/lib/active_job/queue_adapters/resque_ext.rb`.
fn expected_constant(path: &str) -> Option<String> {
    let mount = format!("{}/", super::source_gems::MOUNT);
    let at = path.find(&mount)?;
    if at > 0 && !path[..at].ends_with('/') {
        return None;
    }
    let rest = &path[at + mount.len()..];
    let (_, under_gem) = rest.split_once('/')?;
    let relative = under_gem.strip_prefix("lib/")?.strip_suffix(".rb")?;
    Some(crate::naming::camelize_path(relative))
}

/// Not string equality: the boot read the engine through the app dir it was given (`.` or absolute), the sources were registered under it too, but either may be joined differently.
fn same_file(a: &str, b: &str, dir: &Path) -> bool {
    let norm = |p: &str| {
        let p = Path::new(p);
        let p = if p.is_absolute() { p.to_path_buf() } else { dir.join(p) };
        p.components().filter(|c| !matches!(c, std::path::Component::CurDir)).collect::<std::path::PathBuf>()
    };
    norm(a) == norm(b)
}

fn names(text: &str, leaf: &str) -> bool {
    let bytes = text.as_bytes();
    text.match_indices(leaf).any(|(i, _)| {
        let before = i.checked_sub(1).map(|j| bytes[j]);
        let after = bytes.get(i + leaf.len()).copied();
        !before.is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_')
            && !after.is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_')
    })
}

fn spans(lc: &LibraryClass) -> impl Iterator<Item = Span> + '_ {
    lc.methods
        .iter()
        .map(|m| m.name_span)
        .chain(lc.include_spans.iter().copied())
        .chain(std::iter::once(lc.parent_span))
        .chain(lc.unknown_calls.iter().map(|c| c.span))
        .chain(lc.constants.iter().map(|(_, e)| e.span))
        .filter(|s| !s.is_synthetic())
}

fn file_of(lc: &LibraryClass) -> Option<crate::span::FileId> {
    spans(lc).next().map(|s| s.file)
}

fn first_span(lc: &LibraryClass, file: crate::span::FileId) -> Span {
    spans(lc).find(|s| s.file == file).unwrap_or_else(Span::synthetic)
}
