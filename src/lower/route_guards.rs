//! Request constraints (`RouteSpec::Guarded`) as methods the Ruby-family
//! router asks once a route's path matches: `RoundhouseRouteGuards.guard_<n>(request)`.
//!
//! Rails asks a lambda with the request (`->(req) { req.format == :json }`),
//! an object with `matches?(request)`, and a Hash's request keys with
//! `value === request.<key>`. Each becomes one class method whose body is
//! the constraint's own expressions, so the analyzer types it against
//! `ActionDispatch::Request` and reports what it cannot type where the
//! app wrote it. A target whose router cannot ask them refuses the
//! routes instead (`route_guard_gaps_for_target`).

use crate::dialect::{LibraryClass, MethodDef, MethodReceiver, Param, RouteSpec};
use crate::expr::{Expr, ExprNode};
use crate::ident::{ClassId, Symbol};
use crate::span::Span;

pub const GUARD_CLASS: &str = "RoundhouseRouteGuards";

/// The `constraints` keys Rails answers from the request rather than the
/// path (`ActionDispatch::Request#host`, `#protocol`, …).
pub const REQUEST_KEYS: &[&str] = &["host", "protocol", "subdomain", "domain", "port"];

/// Every request constraint in `entries`, once each, in declaration order:
/// the index of one is its method's number.
pub fn guards(entries: &[RouteSpec]) -> Vec<&Expr> {
    fn walk<'a>(entries: &'a [RouteSpec], out: &mut Vec<&'a Expr>) {
        for entry in entries {
            match entry {
                RouteSpec::Guarded { guard, entries } => {
                    if !out.iter().any(|g| g.span == guard.span) {
                        out.push(guard);
                    }
                    walk(entries, out);
                }
                RouteSpec::Scope { entries, .. } | RouteSpec::Exclusive { entries } => walk(entries, out),
                RouteSpec::Resources { nested, .. } => walk(nested, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(entries, &mut out);
    out
}

/// The guard's position among `guards`, when its method can be written.
pub fn guard_index(entries: &[RouteSpec], guard: &Expr) -> Option<usize> {
    guards(entries).iter().position(|g| g.span == guard.span).filter(|_| guard_body(guard).is_some())
}

/// Whether the Ruby-family router can ask this constraint.
pub fn enforceable(guard: &Expr) -> bool {
    guard_body(guard).is_some()
}

/// The method's parameter and body: the lambda's own, `guard.matches?(request)`,
/// or the Hash's request keys joined with `&&`. None for a constraint that did
/// not ingest (a placeholder `nil`) or a lambda Rails cannot call with one request.
fn guard_body(guard: &Expr) -> Option<(Symbol, Expr)> {
    let request = Symbol::from("request");
    match &*guard.node {
        ExprNode::Lit { value: crate::expr::Literal::Nil } => None,
        ExprNode::Lambda { params, rest_param: None, extra_params, block_param: None, body, .. }
            if params.len() <= 1 && extra_params.is_empty() =>
        {
            Some((params.first().cloned().unwrap_or(request), body.clone()))
        }
        ExprNode::Lambda { .. } => None,
        ExprNode::Hash { entries, .. } => {
            let mut conditions = entries.iter().filter_map(|(key, value)| {
                let ExprNode::Lit { value: crate::expr::Literal::Sym { value: name } } = &*key.node else { return None };
                REQUEST_KEYS.contains(&name.as_str()).then(|| {
                    send(value.span, Some(value.clone()), "===", vec![send(value.span, Some(var(value.span, &request)), name.as_str(), vec![])])
                })
            });
            let first = conditions.next()?;
            let body = conditions.fold(first, |acc, c| {
                Expr::new(acc.span, ExprNode::BoolOp { op: crate::expr::BoolOpKind::And, surface: crate::expr::BoolOpSurface::Symbol, left: acc, right: c })
            });
            Some((request, body))
        }
        _ => Some((request.clone(), send(guard.span, Some(guard.clone()), "matches?", vec![var(guard.span, &request)]))),
    }
}

fn send(span: Span, recv: Option<Expr>, method: &str, args: Vec<Expr>) -> Expr {
    Expr::new(span, ExprNode::Send { recv, method: Symbol::from(method), args, block: None, parenthesized: true })
}

fn var(span: Span, name: &Symbol) -> Expr {
    Expr::new(span, ExprNode::Var { id: crate::ident::VarId(0), name: name.clone() })
}

/// `RoundhouseRouteGuards`, one `guard_<n>` per constraint the router can ask, and the
/// RBS that types each one's request; None when the routes declare none.
pub fn guard_class(entries: &[RouteSpec]) -> Option<(LibraryClass, String)> {
    let mut methods = Vec::new();
    let mut rbs = format!("class {GUARD_CLASS}\n");
    for (index, guard) in guards(entries).into_iter().enumerate() {
        let Some((param, body)) = guard_body(guard) else { continue };
        let name = Symbol::from(format!("guard_{index}"));
        // Named `request` whatever the lambda calls it: the analyzer binds the parameter by position, and an RBS name must be ASCII.
        rbs.push_str(&format!("  def self.{name}: (ActionDispatch::Request request) -> bool\n"));
        methods.push(MethodDef {
            visibility: crate::dialect::MethodVisibility::Public,
            unsupported_formals: None,
            has_anonymous_block: false,
            name_span: Span::synthetic(),
            name,
            receiver: MethodReceiver::Class,
            params: vec![Param::positional(param)],
            body,
            signature: None,
            effects: Default::default(),
            enclosing_class: Some(Symbol::from(GUARD_CLASS)),
            kind: Default::default(),
            is_async: false,
            mutates_self: false,
            block_param: None,
        });
    }
    if methods.is_empty() {
        return None;
    }
    rbs.push_str("end\n");
    let class = LibraryClass {
        name: ClassId(Symbol::from(GUARD_CLASS)),
        is_module: false,
        parent: None,
        parent_span: Default::default(),
        includes: Vec::new(),
        include_spans: Vec::new(),
        methods,
        nullable_columns: Vec::new(),
        origin: None,
        constants: Vec::new(),
        conditional_definitions: Vec::new(),
        unknown_calls: Vec::new(),
        class_ivar_initializers: Vec::new(),
    };
    Some((class, rbs))
}

/// On a target whose router does not ask them, each constraint the Ruby family asks is an error
/// where it is written: its routes are refused, as the ingest refuses one no router can ask.
#[cfg(not(target_arch = "wasm32"))]
pub fn route_guard_gaps_for_target(app: &crate::App, target: crate::build_target::BuildTarget) -> Vec<crate::diagnostic::Diagnostic> {
    if target.asks_route_guards() {
        return Vec::new();
    }
    guards(&app.routes.entries)
        .into_iter()
        .filter(|guard| enforceable(guard))
        .map(|guard| {
            crate::diagnostic::Diagnostic::unsupported(
                guard.span,
                Some(Symbol::from(target.as_str())),
                "request constraint",
                "the routes it guards are not dispatched: this target's router matches by verb and path alone, so it would answer every request the constraint refuses; their URL helpers are still generated",
            )
        })
        .collect()
}
