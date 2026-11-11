//! A constant written from the top level (`::Article`) keeps a leading
//! empty segment in its path (`["", "Article"]`), which the Ruby family
//! and Crystal print as their own `::`. TypeScript, Python and Elixir
//! have no lexical constant lookup for the root to choose against, so
//! the class it names is the bare path's; joined as written, the empty
//! segment would print as a leading `.` (`.Article.total()`).
//!
//! The pass drops the root once, on the target's copy of the app,
//! before any lowering or emitter reads a constant. Every place those
//! emitters name a constant, pick a constructor or collect an import
//! then sees the path a bare `Article` has, and none of them needs a
//! check of its own.

use crate::app::App;
use crate::expr::{Expr, ExprNode, LValue};
use crate::ident::Symbol;
use crate::build_target::BuildTarget;

/// A constant path's segments without the top-level root: `::A::B`'s
/// `["", "A", "B"]` is `["A", "B"]`; an unrooted path is itself.
pub fn const_segments(path: &[Symbol]) -> &[Symbol] {
    match path.split_first() {
        Some((root, rest)) if root.as_str().is_empty() && !rest.is_empty() => rest,
        _ => path,
    }
}

/// True for the targets whose constants have no `::` to print.
fn drops_the_root(target: BuildTarget) -> bool {
    matches!(
        target,
        BuildTarget::Typescript | BuildTarget::TypescriptWorker | BuildTarget::Python | BuildTarget::Elixir
    )
}

/// The app with every rooted constant written bare, for a target that
/// [`drops_the_root`]; `None` when the target keeps `::` or the app has
/// no rooted constant.
pub(crate) fn lower_for_target(app: &App, target: BuildTarget) -> Option<App> {
    if !drops_the_root(target) {
        return None;
    }
    let mut lowered = app.clone();
    let mut changed = false;
    let mut visit = |expr: &mut Expr| unroot(expr, &mut changed);
    crate::lower::for_each_hook_body(&mut lowered, &mut visit);
    crate::lower::for_each_test_body(&mut lowered, &mut visit);
    for view in &mut lowered.views {
        visit(&mut view.body);
    }
    changed.then_some(lowered)
}

fn unroot(expr: &mut Expr, changed: &mut bool) {
    match &mut *expr.node {
        ExprNode::Const { path } => strip(path, changed),
        ExprNode::Assign { target: LValue::Const { path }, .. } => strip(path, changed),
        _ => {}
    }
    expr.node.for_each_child_mut(&mut |child| unroot(child, changed));
}

fn strip(path: &mut Vec<Symbol>, changed: &mut bool) {
    let kept = const_segments(path).len();
    if kept < path.len() {
        path.drain(..path.len() - kept);
        *changed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(segments: &[&str]) -> Vec<Symbol> {
        segments.iter().map(|s| Symbol::from(*s)).collect()
    }

    #[test]
    fn a_rooted_path_drops_its_empty_segment() {
        assert_eq!(const_segments(&path(&["", "A", "B"])), &path(&["A", "B"])[..]);
    }

    #[test]
    fn an_unrooted_path_is_itself() {
        assert_eq!(const_segments(&path(&["A", "B"])), &path(&["A", "B"])[..]);
        assert_eq!(const_segments(&path(&[""])), &path(&[""])[..]);
    }
}
