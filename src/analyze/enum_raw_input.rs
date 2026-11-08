//! The enum columns an app reads `<column>_before_type_cast` on. Rails
//! answers that with what was assigned until the record saves or loads,
//! and the stored value after (activerecord 8.1.4). The ruby family and
//! Spinel keep the assigned value for these columns only; the other
//! targets refuse the reader at the project boundary.

use crate::dialect::MethodReceiver;
use crate::{App, Expr, ExprNode, Symbol};
use std::collections::BTreeSet;

/// Enum columns whose generated reader the app calls. By name: label
/// rewriting runs before receivers are typed. A column is left out when
/// every model declaring it defines the reader itself.
pub(crate) fn raw_input_columns(app: &App) -> BTreeSet<Symbol> {
    if app.models.iter().all(|model| model.enums.is_empty()) {
        return BTreeSet::new();
    }
    fn walk(expr: &Expr, app: &App, out: &mut BTreeSet<Symbol>) {
        if let ExprNode::Send { method, .. } = &*expr.node
            && let Some(column) = method.as_str().strip_suffix("_before_type_cast")
        {
            let column = Symbol::from(column);
            let generated = app.models.iter().any(|m| {
                m.enums.contains_key(&column) && !crate::analyze::source_instance_method(app, &m.name, method.as_str())
            });
            if generated {
                out.insert(column);
            }
        }
        expr.node.for_each_child(&mut |child| walk(child, app, out));
    }
    let mut out = BTreeSet::new();
    crate::lower::for_each_hook_body_ref(app, &mut |body| walk(body, app, &mut out));
    for model in &app.models {
        for method in model.methods().filter(|m| m.receiver == MethodReceiver::Instance) {
            walk(&method.body, app, &mut out);
        }
    }
    for view in &app.views {
        walk(&view.body, app, &mut out);
    }
    for class in &app.library_classes {
        for method in &class.methods {
            walk(&method.body, app, &mut out);
        }
    }
    for module in &app.test_modules {
        for test in &module.tests {
            walk(&test.body, app, &mut out);
        }
    }
    out
}
