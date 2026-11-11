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
//!
//! One case has no bare spelling. TypeScript declares a library class
//! named under a namespace (`Api::Report`) inside `export namespace Api
//! { ... }`, where a bare `Article` is the namespace's own member when
//! `Api::Article` exists; the module's top-level `Article` is hidden.
//! There, the root is what keeps `::Article` on the top-level class,
//! so the constant is refused instead of being retargeted. Python and
//! Elixir declare every class at module level, so a bare name there is
//! always the top-level one.

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

/// Writes every rooted constant of `app` bare, for a target that
/// [`drops_the_root`]; a no-op on any other target. It mutates the
/// target's own copy of the app, the one `project::target_files_inner`
/// already makes for every target outside the Ruby family, so it adds no
/// clone. A constant TypeScript would hide behind a namespace member is
/// refused ([`hidden_by_namespace`]).
pub(crate) fn unroot_for_target(app: &mut App, target: BuildTarget) {
    if !drops_the_root(target) {
        return;
    }
    let declared: Vec<String> = app.library_classes.iter().map(|c| c.name.0.as_str().to_string()).collect();
    let declared: Vec<&str> = declared.iter().map(String::as_str).collect();
    // The classes TypeScript declares inside a namespace block; every
    // other class (a model, a controller) is a module-level declaration.
    let namespaced: Vec<String> = match target {
        BuildTarget::Typescript | BuildTarget::TypescriptWorker => app
            .library_classes
            .iter()
            .filter(|c| crate::emit::typescript::nested_class_root(c).is_some())
            .map(|c| c.name.0.as_str().to_string())
            .collect(),
        _ => Vec::new(),
    };
    crate::lower::for_each_owned_hook_body(app, &mut |owner, expr| {
        let owner = owner.map(|o| o.0.as_str()).filter(|o| namespaced.iter().any(|n| n == o));
        unroot(expr, &mut |path, span| {
            if let Some((owner, scope)) = owner.and_then(|o| Some(o).zip(hidden_by_namespace(o, path, &declared))) {
                let name = const_segments(path).iter().map(|s| s.as_str()).collect::<Vec<_>>().join("::");
                crate::emit::diagnostics::report_unsupported(
                    span,
                    target.as_str(),
                    "top-level constant in a namespace",
                    format!(
                        "`::{name}` in `{owner}`: {} declares `{owner}` inside `namespace {}`, whose own `{}` hides the top-level one, and has no spelling for the top-level class there",
                        target.as_str(),
                        scope.replace("::", "."),
                        path[1].as_str(),
                    ),
                );
            }
            strip(path)
        })
    });
    let mut visit = |expr: &mut Expr| unroot(expr, &mut |path, _| strip(path));
    crate::lower::for_each_test_body(app, &mut visit);
    for view in &mut app.views {
        visit(&mut view.body);
    }
}

/// For a rooted `path` read in `owner`'s body, the enclosing namespace
/// (`Api` for `Api::Report`) that declares a class or namespace named
/// like the path's first segment, which a bare name would reach first.
fn hidden_by_namespace<'a>(owner: &'a str, path: &[Symbol], declared: &[&str]) -> Option<&'a str> {
    let first = const_segments(path).first()?.as_str();
    if const_segments(path).len() == path.len() {
        return None;
    }
    let mut scopes: Vec<&str> = owner.match_indices("::").map(|(i, _)| &owner[..i]).collect();
    scopes.reverse();
    scopes.into_iter().find(|scope| {
        let member = format!("{scope}::{first}");
        declared.iter().any(|d| *d == member || d.starts_with(&format!("{member}::")))
    })
}

fn unroot(expr: &mut Expr, on_path: &mut impl FnMut(&mut Vec<Symbol>, crate::span::Span)) {
    let span = expr.span;
    match &mut *expr.node {
        ExprNode::Const { path } => on_path(path, span),
        ExprNode::Assign { target: LValue::Const { path }, .. } => on_path(path, span),
        _ => {}
    }
    expr.node.for_each_child_mut(&mut |child| unroot(child, on_path));
}

fn strip(path: &mut Vec<Symbol>) {
    let kept = const_segments(path).len();
    path.drain(..path.len() - kept);
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
    fn a_namespace_member_of_the_same_name_hides_the_top_level_class() {
        let declared = ["Api::Article", "Api::Report", "Admin::Tally::Row"];
        let rooted = path(&["", "Article"]);
        assert_eq!(hidden_by_namespace("Api::Report", &rooted, &declared), Some("Api"));
        assert_eq!(hidden_by_namespace("Api::V1::Report", &rooted, &declared), Some("Api"));
        assert_eq!(hidden_by_namespace("Admin::Report", &path(&["", "Tally"]), &declared), Some("Admin"));
        assert_eq!(hidden_by_namespace("Admin::Report", &rooted, &declared), None);
        assert_eq!(hidden_by_namespace("Report", &rooted, &declared), None);
        assert_eq!(hidden_by_namespace("Api::Report", &path(&["Article"]), &declared), None);
    }

    #[test]
    fn an_unrooted_path_is_itself() {
        assert_eq!(const_segments(&path(&["A", "B"])), &path(&["A", "B"])[..]);
        assert_eq!(const_segments(&path(&[""])), &path(&[""])[..]);
    }
}
