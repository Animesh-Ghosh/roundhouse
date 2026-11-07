//! What a gem's code does that the generic passes cannot model, written
//! here once per gem rather than in the passes. Each hook is keyed on the
//! gem's own names and applies only where they stand, so an app needs no
//! setting for it.

use crate::App;
use crate::det::{HashMap, HashSet};
use crate::expr::{Expr, ExprNode, Literal};

use super::configured_superclass::Val;

/// What the evaluator does with a call on the routing mapper in place of recording it.
pub(super) enum RouterCall {
    /// Runs the call's block as though the call were not there.
    RunBlock,
}

/// `caller` is the program's method the call is made from, as (owner, name).
pub(super) fn router_call(known: &HashSet<String>, caller: Option<(&str, &str)>, method: &str, argv: &[Val]) -> Option<RouterCall> {
    // devise's `devise_scope` wraps its block in `constraints(lambda { |request| request.env["devise.mapping"] = …; true })`: always true, and the env it sets is read only by devise's controllers, whose mapping is not modeled.
    if known.contains("Devise")
        && caller == Some(("ActionDispatch::Routing::Mapper", "devise_scope"))
        && method == "constraints"
        && matches!(argv, [Val::Proc { .. }])
    {
        return Some(RouterCall::RunBlock);
    }
    None
}

/// What the hooks read off the app once.
#[derive(Default)]
pub(super) struct Facts {
    /// Each model's `devise :a, :b` modules, by class name.
    devise_models: HashMap<String, Vec<String>>,
    /// The models whose `devise` call also passes options (`unlock_strategy: :time`), which set that model's own config.
    devise_configured: HashSet<String>,
}

impl Facts {
    pub(super) fn of(app: &App) -> Self {
        let mut devise_configured = HashSet::default();
        for model in &app.models {
            let configured = model.body.iter().any(|item| match item {
                crate::dialect::ModelBodyItem::Unknown { expr, .. } => matches!(&*expr.node, ExprNode::Send { recv: None, method, args, .. }
                    if method.as_str() == "devise" && args.iter().any(|a| !matches!(&*a.node, ExprNode::Lit { value: Literal::Sym { .. } }))),
                _ => false,
            });
            if configured {
                devise_configured.insert(model.name.0.as_str().to_string());
            }
        }
        Facts { devise_models: devise_modules(app), devise_configured }
    }

    /// Whether `class_answer` may answer for `class`.
    pub(super) fn answers_for(&self, class: &str) -> bool {
        self.devise_models.contains_key(class.trim_start_matches("::"))
    }
}

/// Each model's `devise :a, :b` declaration, by class name, as `Devise::Models#devise` reads it.
pub(super) fn devise_modules(app: &App) -> HashMap<String, Vec<String>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::default();
    for model in &app.models {
        for item in &model.body {
            let crate::dialect::ModelBodyItem::Unknown { expr, .. } = item else { continue };
            let ExprNode::Send { recv: None, method, args, .. } = &*expr.node else { continue };
            if method.as_str() != "devise" {
                continue;
            }
            let modules = out.entry(model.name.0.as_str().to_string()).or_default();
            for arg in args {
                if let ExprNode::Lit { value: Literal::Sym { value } } = &*arg.node {
                    modules.push(value.as_str().to_string());
                }
            }
        }
    }
    out
}

/// What a class outside the library classes (a model) answers that a gem's macro gave it.
/// `all` is `Devise::ALL` as the evaluation holds it, the order `devise` keeps its modules in.
/// `config` reads a setting of the gem's (`Devise.unlock_strategy`).
pub(super) fn class_answer(facts: &Facts, class: &str, method: &str, argv: &[Val], all: Option<&[Val]>, config: &mut dyn FnMut(&str) -> Option<Val>) -> Option<Val> {
    // `Devise::Models#devise`, which Devise extends onto every model: it records the modules as `devise_modules`, sorted by `Devise::ALL`.
    let class = class.trim_start_matches("::");
    let modules = facts.devise_models.get(class)?;
    match (method, argv) {
        // `Devise::Models::Lockable::ClassMethods`: the model's `unlock_strategy` is Devise's unless its own `devise` call sets one.
        ("unlock_strategy_enabled?", [Val::Sym(strategy)]) if modules.iter().any(|m| m == "lockable") && !facts.devise_configured.contains(class) => {
            let Val::Sym(set) = config("unlock_strategy")? else { return None };
            Some(Val::Bool(set == *strategy || set == "both"))
        }
        ("respond_to?", [Val::Sym(name), ..]) if matches!(name.as_str(), "devise" | "devise_modules") => Some(Val::Bool(true)),
        ("devise_modules", []) => {
            let mut selected: Vec<String> = Vec::new();
            for m in modules {
                if !selected.contains(m) {
                    selected.push(m.clone());
                }
            }
            let rank = |m: &String| all.and_then(|all| all.iter().position(|a| matches!(a, Val::Sym(s) if s == m))).map_or(-1, |i| i as i64);
            selected.sort_by_key(rank);
            Some(Val::Array(selected.into_iter().map(Val::Sym).collect()))
        }
        _ => None,
    }
}

/// A statement of a gem's method the evaluation passes over without running it, and that writes nothing it reads.
pub(super) fn skips(known: &HashSet<String>, caller: Option<(&str, &str)>, statement: &Expr) -> bool {
    // `Devise.add_mapping`'s `@@helpers.each { |h| h.define_helpers(mapping) }`: each helper module `class_eval`s the mapping's controller helpers (`authenticate_user!`), which no route reads.
    known.contains("Devise")
        && caller == Some(("Devise", "add_mapping"))
        && matches!(&*statement.node, ExprNode::Send { method, block: Some(_), .. } if method.as_str() == "each")
        && crate::emit::ruby::emit_expr(statement).contains("define_helpers(mapping)")
}

/// A top-level statement that only registers its block for later, so replaying the files runs none of it.
pub(super) fn registers_for_later(statement: &Expr) -> bool {
    // Warden's `Manager.after_set_user { … }` and the other `Warden::Hooks`: each appends its block to a list Warden runs per request.
    let ExprNode::Send { recv: Some(recv), method, block: Some(_), .. } = &*statement.node else { return false };
    let ExprNode::Const { path } = &*recv.node else { return false };
    let written: Vec<&str> = path.iter().map(|s| s.as_str()).filter(|s| !s.is_empty()).collect();
    written == ["Warden", "Manager"]
        && matches!(
            method.as_str(),
            "after_set_user" | "after_authentication" | "after_fetch" | "after_failed_fetch" | "before_failure" | "before_logout" | "on_request"
                | "prepend_after_set_user" | "prepend_after_authentication" | "prepend_after_fetch" | "prepend_before_failure" | "prepend_before_logout" | "prepend_on_request"
        )
}
