// Not reported as an error: every call of such a method was expanded where it loads (`library_class::inline_load_time_definers`), so its run-time definition is never reached.
use crate::app::App;
use crate::expr::{Expr, ExprNode};
use crate::ident::Symbol;

pub(super) fn mark(app: &mut App) {
    let definers: Vec<Symbol> = app
        .library_classes
        .iter()
        .flat_map(|lc| lc.methods.iter())
        .filter(|m| only_stamps(&m.body))
        .map(|m| m.name.clone())
        .collect();
    if definers.is_empty() {
        return;
    }
    let mut called: crate::det::HashSet<Symbol> = crate::det::HashSet::default();
    crate::lower::for_each_hook_body_ref(app, &mut |e| calls(e, &definers, &mut called));
    for lc in app.library_classes.iter_mut() {
        for m in lc.methods.iter_mut() {
            if definers.contains(&m.name) && !called.contains(&m.name) && only_stamps(&m.body) {
                stamp(&mut m.body);
            }
        }
    }
}

fn is_stamp(e: &Expr) -> bool {
    matches!(e.diagnostic(), Some(crate::diagnostic::DiagnosticKind::Unsupported { construct, .. }) if construct.as_str() == crate::diagnostic::NESTED_DEF)
}

fn only_stamps(e: &Expr) -> bool {
    match &*e.node {
        ExprNode::Seq { exprs } => !exprs.is_empty() && exprs.iter().all(is_stamp),
        _ => is_stamp(e),
    }
}

fn stamp(e: &mut Expr) {
    match &mut *e.node {
        ExprNode::Seq { exprs } => exprs.iter_mut().for_each(|x| x.decisions |= crate::expr::UNCALLED_DEFINER),
        _ => e.decisions |= crate::expr::UNCALLED_DEFINER,
    }
}

fn calls(e: &Expr, names: &[Symbol], out: &mut crate::det::HashSet<Symbol>) {
    if let ExprNode::Send { method, .. } = &*e.node
        && names.contains(method)
    {
        out.insert(method.clone());
    }
    e.node.for_each_child(&mut |c| calls(c, names, out));
}
