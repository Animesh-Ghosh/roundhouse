//! A superclass read off a configured object as the class loads, as
//! doorkeeper 5.9 writes its controllers' parents:
//!
//! ```ruby
//! class ApplicationController < Doorkeeper.config.resolve_controller(:base)
//! ```
//!
//! Rails has run the app's initializers by then, and the host's
//! `Doorkeeper.configure do base_controller "ApplicationController" end`
//! has set the option the expression reads. Ingest records such an
//! expression ([`defer`]) instead of refusing it, and [`fold`] evaluates
//! it once the class macros have expanded (an option DSL's readers and
//! the builder's writers are ordinary methods by then).
//!
//! The evaluation is over the program's own methods, on a small heap of
//! objects: the initializer's calls on the expression's root constant
//! first, then the expression. A built-in it does not model, called on a
//! value that is not the program's (`1.year`, `Rails.env`), answers a
//! value that is carried but never tested or called on. A statement whose
//! value is not used and that it cannot run is passed over: every
//! instance variable a method it can reach may write is unsure until the
//! evaluation writes it again, and reading an unsure one stops the
//! evaluation. What stops leaves the superclass refused, as before.

use std::cell::RefCell;
use std::path::Path;

use crate::App;
use crate::det::{HashMap, HashSet};
use crate::dialect::{LibraryClass, MethodDef, MethodReceiver};
use crate::expr::{BoolOpKind, Expr, ExprNode, InterpPart, LValue, Literal, OpAssignOp};
use crate::ident::{ClassId, Symbol};
use crate::span::Span;
use crate::vfs::Vfs;

use super::util::constant_path_of;

struct Pending {
    owner: ClassId,
    expr: Expr,
    span: Span,
}

thread_local! {
    static PENDING: RefCell<Vec<Pending>> = const { RefCell::new(Vec::new()) };
}

pub(super) fn reset() {
    PENDING.with(|p| p.borrow_mut().clear());
}

/// Records `node` for [`fold`] when it is a call chain rooted at a
/// constant (`Doorkeeper.config.resolve_controller(:base)`); `false`
/// leaves the superclass to the caller's refusal.
pub(super) fn defer(owner: &ClassId, node: &ruby_prism::Node<'_>, span: Span, file: &str) -> bool {
    let mut root = node.as_call_node();
    let mut calls = 0;
    while let Some(call) = root {
        calls += 1;
        match call.receiver() {
            Some(r) if r.as_call_node().is_some() => root = r.as_call_node(),
            Some(r) if constant_path_of(&r).is_some() => break,
            _ => return false,
        }
    }
    if calls == 0 {
        return false;
    }
    let Ok(expr) = super::expr::ingest_expr(node, file) else { return false };
    PENDING.with(|p| {
        let mut p = p.borrow_mut();
        if !p.iter().any(|d| &d.owner == owner) {
            p.push(Pending { owner: owner.clone(), expr, span });
        }
    });
    true
}

/// Each deferred superclass: the constant it evaluates to, set as the
/// class's parent, or the refusal ingest would have reported.
pub(super) fn fold<V: Vfs + ?Sized>(vfs: &V, dir: &Path, app: &mut App) {
    let pending = PENDING.with(|p| std::mem::take(&mut *p.borrow_mut()));
    if pending.is_empty() {
        return;
    }
    let initializers = initializer_statements(vfs, dir);
    for item in pending {
        let answer = Eval::new(app).answer(&item.expr, &initializers);
        match answer {
            Ok(name) => {
                let parent = ClassId(Symbol::from(name.as_str()));
                for controller in app.controllers.iter_mut().filter(|c| c.name == item.owner) {
                    controller.parent = Some(parent.clone());
                }
                for lc in app.library_classes.iter_mut().filter(|c| c.name == item.owner) {
                    lc.parent = Some(parent.clone());
                }
            }
            Err(_) => app.superclass_refusals.push(crate::diagnostic::Diagnostic::unsupported(
                item.span,
                None,
                "superclass",
                format!(
                    "`{}`'s superclass is an expression evaluated when the class loads, not a constant, so the class is emitted without it",
                    item.owner.0.as_str()
                ),
            )),
        }
    }
}

/// The top-level statements of `config/initializers/*.rb`, in the order Rails loads them.
fn initializer_statements<V: Vfs + ?Sized>(vfs: &V, dir: &Path) -> Vec<Expr> {
    let root = dir.join("config/initializers");
    if !vfs.is_dir(&root) {
        return Vec::new();
    }
    let mut files = super::app::read_rb_files(vfs, &root).unwrap_or_default();
    files.sort();
    let mut out = Vec::new();
    for file in files {
        let Ok(source) = vfs.read(&file) else { continue };
        let name = file.display().to_string();
        let parsed = super::prism::parse_silent(&source);
        let node = parsed.node();
        let Some(program) = node.as_program_node() else { continue };
        for statement in program.statements().body().iter() {
            let (read, _) = super::prism::scope(|| super::expr::ingest_expr(&statement, &name));
            if let Ok(expr) = read {
                out.push(expr);
            }
        }
    }
    out
}

#[derive(Clone, Debug)]
enum Val {
    Nil,
    Bool(bool),
    Int(i64),
    Str(String),
    Sym(String),
    /// A class or module, by full name; one the program does not define is named only.
    Class(String),
    Obj(usize),
    Array(Vec<Val>),
    Hash(Vec<(Val, Val)>),
    Proc { params: Vec<Symbol>, rest: Option<Symbol>, body: Expr, scope: Option<String> },
    Regex(String),
    /// What a built-in answered that this does not model: carried, never tested or called on.
    Unknown,
}

impl Val {
    fn truthy(&self) -> Res<bool> {
        match self {
            Val::Unknown => Err("a value it does not model is tested".into()),
            v => Ok(!matches!(v, Val::Nil | Val::Bool(false))),
        }
    }
}

struct Object {
    class: String,
    ivars: HashMap<String, Val>,
}

/// Where a method body runs: its `self`, its locals, its block and the class its constants resolve in.
#[derive(Clone)]
struct Frame {
    this: Val,
    locals: HashMap<Symbol, Val>,
    block: Option<Val>,
    scope: Option<String>,
}

enum Flow {
    Value(Val),
    Return(Val),
}

type Res<T> = Result<T, String>;

/// An instance variable by the class whose objects (or, class side, which class itself) hold it; `*` for any.
type Tag = (String, bool, String);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Holder {
    Object(usize),
    Class(String),
}

const MAX_DEPTH: usize = 40;

struct Eval<'a> {
    classes: &'a [LibraryClass],
    known: HashSet<String>,
    heap: Vec<Object>,
    /// A class or module's own instance variables (`Doorkeeper`'s `@config`).
    class_ivars: HashMap<(String, String), Val>,
    /// When a statement passed over may last have written names of a holder (a pattern: `*_secret_strategy`, `*`), and when the evaluation last wrote each name.
    unsure: HashMap<Holder, Vec<(String, usize)>>,
    written: HashMap<(Holder, String), usize>,
    tick: usize,
    depth: usize,
}

impl<'a> Eval<'a> {
    fn new(app: &'a App) -> Self {
        let known = app.library_classes.iter().map(|c| c.name.0.as_str().to_string()).collect();
        Eval { classes: &app.library_classes, known, heap: Vec::new(), class_ivars: HashMap::default(), unsure: HashMap::default(), written: HashMap::default(), tick: 0, depth: 0 }
    }

    /// The constant `expr` names once the initializers that call on its root have run.
    fn answer(&mut self, expr: &Expr, initializers: &[Expr]) -> Res<String> {
        let root = root_constant(expr).ok_or("no constant at the root")?;
        let top = Frame { this: Val::Class("Object".into()), locals: HashMap::default(), block: None, scope: None };
        for statement in initializers.iter().filter(|s| root_constant(s).as_deref() == Some(root.as_str())) {
            self.statement(statement, &mut top.clone())?;
        }
        let value = self.value(expr, &mut top.clone())?;
        match value {
            Val::Class(name) => Ok(name),
            other => Err(format!("it evaluates to {other:?}, not a class")),
        }
    }

    /// A statement whose value is not used: one it cannot run is passed over and its possible writes kept.
    fn statement(&mut self, e: &Expr, frame: &mut Frame) -> Res<Option<Val>> {
        match self.eval(e, frame) {
            Ok(Flow::Return(v)) => Ok(Some(v)),
            Ok(Flow::Value(_)) => Ok(None),
            Err(why) if !matches!(&*e.node, ExprNode::Return { .. }) => {
                let (owner, side) = self.tag_of(&frame.this);
                let Some(writes) = self.may_write(e, &owner, side, &mut HashSet::default()) else {
                    return Err(format!("{why}, in a statement whose writes are not known"));
                };
                for tag in &writes {
                    self.make_unsure(tag);
                }
                // A local it may have assigned is no longer what the frame holds.
                visit(e, &mut |x| {
                    if let ExprNode::Assign { target: LValue::Var { name, .. }, .. } | ExprNode::OpAssign { target: LValue::Var { name, .. }, .. } = &*x.node {
                        frame.locals.remove(name);
                    }
                });
                Ok(None)
            }
            Err(why) => Err(why),
        }
    }

    fn tag_of(&self, this: &Val) -> (String, bool) {
        match this {
            Val::Obj(i) => (self.heap[*i].class.clone(), false),
            Val::Class(c) => (c.clone(), true),
            _ => ("*".into(), false),
        }
    }

    fn make_unsure(&mut self, (owner, side, name): &Tag) {
        self.tick += 1;
        let mut holders: Vec<Holder> = (0..self.heap.len())
            .filter(|&i| !*side && (owner == "*" || self.ancestry(&self.heap[i].class).contains(owner)))
            .map(Holder::Object)
            .collect();
        if *side || owner == "*" {
            if owner == "*" {
                holders.extend(self.known.iter().cloned().map(Holder::Class));
            } else {
                holders.push(Holder::Class(owner.clone()));
            }
        }
        for h in holders {
            self.unsure.entry(h).or_default().push((name.clone(), self.tick));
        }
    }

    fn holder(this: &Val) -> Option<Holder> {
        match this {
            Val::Obj(i) => Some(Holder::Object(*i)),
            Val::Class(c) => Some(Holder::Class(c.clone())),
            _ => None,
        }
    }

    fn note_read(&mut self, this: &Val, name: String) -> Res<()> {
        let Some(h) = Self::holder(this) else { return Ok(()) };
        let last_unsure = self.unsure.get(&h).into_iter().flatten().filter(|(p, _)| glob(p, &name)).map(|(_, t)| *t).max();
        let written = self.written.get(&(h, name.clone())).copied().unwrap_or(0);
        if last_unsure.is_some_and(|u| u > written) { Err(format!("`@{name}` may have been written by a statement passed over")) } else { Ok(()) }
    }

    /// The class and what its lookup reaches: superclasses and included modules.
    fn ancestry(&self, class: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut todo = vec![class.to_string()];
        while let Some(c) = todo.pop() {
            if out.contains(&c) {
                continue;
            }
            out.push(c.clone());
            for decl in self.classes.iter().filter(|d| d.name.0.as_str() == c) {
                todo.extend(decl.includes.iter().filter_map(|i| self.resolve(i.0.as_str(), Some(&c))));
                todo.extend(decl.parent.iter().filter_map(|p| self.resolve(p.0.as_str(), Some(&c))));
            }
        }
        out
    }

    fn value(&mut self, e: &Expr, frame: &mut Frame) -> Res<Val> {
        match self.eval(e, frame)? {
            Flow::Value(v) | Flow::Return(v) => Ok(v),
        }
    }

    fn eval(&mut self, e: &Expr, frame: &mut Frame) -> Res<Flow> {
        let v = match &*e.node {
            ExprNode::Seq { exprs } => {
                let Some((last, rest)) = exprs.split_last() else { return Ok(Flow::Value(Val::Nil)) };
                for st in rest {
                    if let Some(v) = self.statement(st, frame)? {
                        return Ok(Flow::Return(v));
                    }
                }
                return self.eval(last, frame);
            }
            ExprNode::Return { value } => return Ok(Flow::Return(self.value(value, frame)?)),
            ExprNode::Lit { value } => literal(value)?,
            ExprNode::SelfRef => frame.this.clone(),
            ExprNode::Var { name, .. } => frame.locals.get(name).cloned().ok_or_else(|| format!("`{}` is not bound", name.as_str()))?,
            ExprNode::Ivar { name } => {
                self.note_read(&frame.this, name.as_str().to_string())?;
                self.ivar(&frame.this, name.as_str()).unwrap_or(Val::Nil)
            }
            ExprNode::Const { path } => self.constant(path, frame.scope.as_deref())?,
            ExprNode::StringInterp { parts } => {
                let mut text = String::new();
                for part in parts {
                    match part {
                        InterpPart::Text { value } => text.push_str(value),
                        InterpPart::Expr { expr } => {
                            let v = self.value(expr, frame)?;
                            text.push_str(&self.text(&v)?);
                        }
                    }
                }
                Val::Str(text)
            }
            ExprNode::Array { elements, .. } => {
                Val::Array(elements.iter().map(|el| self.value(el, frame)).collect::<Res<_>>()?)
            }
            ExprNode::Hash { entries, .. } => Val::Hash(
                entries.iter().map(|(k, v)| Ok((self.value(k, frame)?, self.value(v, frame)?))).collect::<Res<_>>()?,
            ),
            ExprNode::BoolOp { op, left, right, .. } => {
                let l = self.value(left, frame)?;
                if l.truthy()? == matches!(op, BoolOpKind::Or) { l } else { self.value(right, frame)? }
            }
            ExprNode::If { cond, then_branch, else_branch } => {
                return if self.value(cond, frame)?.truthy()? { self.eval(then_branch, frame) } else { self.eval(else_branch, frame) };
            }
            ExprNode::Defined { operand } => match &*operand.node {
                ExprNode::Ivar { name } => {
                    self.note_read(&frame.this, name.as_str().to_string())?;
                    match self.ivar(&frame.this, name.as_str()) {
                        Some(_) => Val::Str("instance-variable".into()),
                        None => Val::Nil,
                    }
                }
                _ => return Err("a `defined?` of other than an instance variable".into()),
            },
            ExprNode::Assign { target, value } => {
                let v = self.value(value, frame)?;
                match target {
                    LValue::Var { name, .. } => {
                        frame.locals.insert(name.clone(), v.clone());
                    }
                    LValue::Ivar { name } => self.set_ivar(&frame.this, name.as_str(), v.clone())?,
                    _ => return Err("an assignment to other than a local or an instance variable".into()),
                }
                v
            }
            ExprNode::OpAssign { target, op: OpAssignOp::OrOr, value } => {
                let current = match target {
                    LValue::Var { name, .. } => frame.locals.get(name).cloned(),
                    LValue::Ivar { name } => {
                        self.note_read(&frame.this, name.as_str().to_string())?;
                        self.ivar(&frame.this, name.as_str())
                    }
                    _ => return Err("an `||=` on other than a local or an instance variable".into()),
                };
                let set = match &current {
                    Some(v) => v.truthy()?,
                    None => false,
                };
                match current {
                    Some(v) if set => v,
                    _ => {
                        let v = self.value(value, frame)?;
                        match target {
                            LValue::Var { name, .. } => {
                                frame.locals.insert(name.clone(), v.clone());
                            }
                            LValue::Ivar { name } => self.set_ivar(&frame.this, name.as_str(), v.clone())?,
                            _ => unreachable!(),
                        }
                        v
                    }
                }
            }
            ExprNode::Lambda { params, rest_param, extra_params, body, .. } if extra_params.is_empty() => {
                // The block's own `self` is the frame's; `instance_exec` binds another.
                Val::Proc { params: params.clone(), rest: rest_param.clone(), body: body.clone(), scope: frame.scope.clone() }
            }
            ExprNode::Send { recv, method, args, block, .. } => self.send(recv.as_ref(), method.as_str(), args, block.as_ref(), frame)?,
            // Not a rescue it can take: nothing here raises an exception it models, so one that would stops the evaluation.
            ExprNode::BeginRescue { body, else_branch, ensure, .. } => {
                let mut v = self.value(body, frame)?;
                if let Some(e) = else_branch {
                    v = self.value(e, frame)?;
                }
                if let Some(e) = ensure {
                    self.value(e, frame)?;
                }
                v
            }
            _ => return Err(format!("it cannot run {}", e.node.kind_str())),
        };
        Ok(Flow::Value(v))
    }

    fn send(&mut self, recv: Option<&Expr>, method: &str, args: &[Expr], block: Option<&Expr>, frame: &mut Frame) -> Res<Val> {
        let this = match recv {
            Some(r) => self.value(r, frame)?,
            None => frame.this.clone(),
        };
        let mut argv = Vec::new();
        for a in args {
            match &*a.node {
                ExprNode::Splat { value } => match self.value(value, frame)? {
                    Val::Array(items) => argv.extend(items),
                    _ => return Err("a splat of other than an Array".into()),
                },
                _ => argv.push(self.value(a, frame)?),
            }
        }
        let blk = match block {
            None => None,
            Some(b) => Some(self.value(b, frame)?).filter(|v| !matches!(v, Val::Nil)),
        };
        // `@scopes.push(*names)`: the Array the ivar or local holds, grown in place.
        if matches!(method, "push" | "<<" | "concat" | "append" | "uniq!" | "compact!" | "flatten!")
            && let Val::Array(items) = &this
            && let Some(r) = recv
            && matches!(&*r.node, ExprNode::Ivar { .. } | ExprNode::Var { .. })
            && !argv.iter().any(|a| matches!(a, Val::Unknown))
        {
            let mut grown = items.clone();
            let changed = match (method, argv.as_slice()) {
                ("concat", [Val::Array(more)]) => {
                    grown.extend(more.iter().cloned());
                    true
                }
                ("concat", _) => return Err("a `concat` of other than an Array".into()),
                ("uniq!", []) => {
                    let before = grown.len();
                    let mut kept: Vec<Val> = Vec::new();
                    for v in grown {
                        if !kept.iter().any(|k| same(k, &v)) {
                            kept.push(v);
                        }
                    }
                    grown = kept;
                    grown.len() != before
                }
                ("compact!", []) => {
                    let before = grown.len();
                    grown.retain(|v| !matches!(v, Val::Nil));
                    grown.len() != before
                }
                ("flatten!", []) => {
                    let mut flat = Vec::new();
                    let mut nested = false;
                    for v in grown {
                        match v {
                            Val::Array(inner) => {
                                nested = true;
                                flat.extend(inner);
                            }
                            v => flat.push(v),
                        }
                    }
                    grown = flat;
                    nested
                }
                (_, []) if method.ends_with('!') => return Err(format!("`{method}` with arguments")),
                _ => {
                    grown.extend(argv.iter().cloned());
                    true
                }
            };
            let grown = Val::Array(grown);
            match &*r.node {
                ExprNode::Ivar { name } => self.set_ivar(&frame.this, name.as_str(), grown.clone())?,
                ExprNode::Var { name, .. } => {
                    frame.locals.insert(name.clone(), grown.clone());
                }
                _ => unreachable!(),
            }
            // Ruby's bang forms answer nil when nothing changed.
            return Ok(if method.ends_with('!') && !changed { Val::Nil } else { grown });
        }
        // A method the program defines answers before anything built in.
        if let Some((def, scope)) = self.lookup(&this, method) {
            return self.invoke(&def, scope, this, argv, blk);
        }
        // Not answered from a value it does not model: `nil?` or `==` on one would guess.
        let stores = method == "instance_variable_set" && matches!(argv.first(), Some(Val::Sym(_)));
        if matches!(this, Val::Unknown) || (!stores && argv.iter().any(|a| matches!(a, Val::Unknown))) {
            return Err(format!("`{method}` on or with a value it does not model"));
        }
        match (method, &this, argv.as_slice()) {
            ("block_given?", _, []) if recv.is_none() => Ok(Val::Bool(frame.block.is_some())),
            ("lambda" | "proc", _, []) if recv.is_none() => blk.ok_or_else(|| format!("`{method}` without a block")),
            ("new", Val::Class(name), _) if self.known.contains(name) => {
                self.heap.push(Object { class: name.clone(), ivars: HashMap::default() });
                let obj = Val::Obj(self.heap.len() - 1);
                if let Some((def, scope)) = self.lookup(&obj, "initialize") {
                    self.invoke(&def, scope, obj.clone(), argv, blk)?;
                }
                Ok(obj)
            }
            ("instance_eval" | "instance_exec", _, _) => {
                let Some(Val::Proc { params, rest, body, scope }) = blk else { return Err(format!("`{method}` without a block")) };
                let mut inner = Frame { this, locals: HashMap::default(), block: None, scope };
                bind(&mut inner.locals, &params, rest.as_ref(), argv);
                self.value(&body, &mut inner)
            }
            ("call", Val::Proc { params, rest, body, scope }, _) => {
                let mut inner = Frame { this: frame.this.clone(), locals: frame.locals.clone(), block: None, scope: scope.clone() };
                bind(&mut inner.locals, params, rest.as_ref(), argv);
                self.value(body, &mut inner)
            }
            ("instance_variable_set", Val::Obj(_), [Val::Sym(name), value]) => {
                let bare = name.strip_prefix('@').ok_or("an ivar name without `@`")?;
                self.set_ivar(&this, bare, value.clone())?;
                Ok(value.clone())
            }
            ("instance_variable_get", _, [Val::Sym(name)]) => {
                let bare = name.strip_prefix('@').ok_or("an ivar name without `@`")?;
                self.note_read(&this, bare.to_string())?;
                Ok(self.ivar(&this, bare).unwrap_or(Val::Nil))
            }
            ("public_send" | "send" | "__send__", _, [Val::Sym(name), ..]) => {
                let rest = argv[1..].to_vec();
                match self.lookup(&this, name) {
                    Some((def, scope)) => self.invoke(&def, scope, this, rest, blk),
                    None => Err(format!("no `{name}` to send")),
                }
            }
            ("respond_to?", _, [Val::Sym(name), ..]) => Ok(Val::Bool(
                self.lookup(&this, name).is_some() || (matches!(this, Val::Proc { .. }) && name == "call"),
            )),
            ("constantize", Val::Str(name), []) => Ok(Val::Class(name.trim_start_matches("::").to_string())),
            ("to_s", _, []) => Ok(Val::Str(self.text(&this)?)),
            ("to_sym", Val::Str(s) | Val::Sym(s), []) => Ok(Val::Sym(s.clone())),
            ("nil?", _, []) => Ok(Val::Bool(matches!(this, Val::Nil))),
            ("!", _, []) => Ok(Val::Bool(!this.truthy()?)),
            ("first", Val::Array(items), []) => Ok(items.first().cloned().unwrap_or(Val::Nil)),
            ("name", Val::Class(c), []) => Ok(Val::Str(c.clone())),
            ("blank?" | "present?" | "presence", _, []) => {
                let blank = match &this {
                    Val::Nil | Val::Bool(false) => true,
                    Val::Str(t) => t.trim().is_empty(),
                    Val::Array(items) => items.is_empty(),
                    Val::Hash(entries) => entries.is_empty(),
                    Val::Bool(true) | Val::Int(_) | Val::Sym(_) | Val::Class(_) | Val::Proc { .. } | Val::Regex(_) => false,
                    Val::Obj(_) | Val::Unknown => return Err(format!("`{method}` on an object")),
                };
                Ok(match method {
                    "blank?" => Val::Bool(blank),
                    "present?" => Val::Bool(!blank),
                    _ => if blank { Val::Nil } else { this.clone() },
                })
            }
            ("classify", Val::Str(t), []) => Ok(Val::Str(crate::naming::classify_path(t))),
            ("camelize", Val::Str(t), []) => Ok(Val::Str(crate::naming::camelize_path(t))),
            ("=~" | "match?", Val::Str(t), [Val::Regex(p)]) | ("=~" | "match?", Val::Regex(p), [Val::Str(t)]) => {
                let re = regex::Regex::new(p).map_err(|_| "a pattern Rust's regex does not read".to_string())?;
                let at = re.find(t).map(|m| t[..m.start()].chars().count() as i64);
                Ok(match (method, at) {
                    ("match?", at) => Val::Bool(at.is_some()),
                    (_, Some(i)) => Val::Int(i),
                    (_, None) => Val::Nil,
                })
            }
            ("include?", Val::Array(items), [x]) => Ok(Val::Bool(items.iter().any(|i| same(i, x)))),
            ("exclude?", Val::Array(items), [x]) => Ok(Val::Bool(!items.iter().any(|i| same(i, x)))),
            ("empty?", Val::Array(items), []) => Ok(Val::Bool(items.is_empty())),
            ("empty?", Val::Str(t), []) => Ok(Val::Bool(t.is_empty())),
            ("freeze" | "dup", _, []) => Ok(this.clone()),
            ("tap", _, []) => {
                let Some(proc_) = blk.clone() else { return Err("`tap` without a block".into()) };
                self.call_proc(&proc_, vec![this.clone()], frame)?;
                Ok(this.clone())
            }
            ("all?" | "any?" | "none?" | "each" | "map" | "select" | "reject", Val::Array(items), []) => {
                let Some(proc_) = blk.clone() else { return Err(format!("`{method}` without a block")) };
                let mut answers = Vec::new();
                for item in items.clone() {
                    answers.push(self.call_proc(&proc_, vec![item], frame)?);
                }
                let truths: Vec<bool> = answers.iter().map(Val::truthy).collect::<Res<_>>()?;
                Ok(match method {
                    "all?" => Val::Bool(truths.iter().all(|t| *t)),
                    "any?" => Val::Bool(truths.iter().any(|t| *t)),
                    "none?" => Val::Bool(!truths.iter().any(|t| *t)),
                    "each" => this.clone(),
                    "map" => Val::Array(answers),
                    "select" => Val::Array(items.iter().zip(&truths).filter(|(_, t)| **t).map(|(i, _)| i.clone()).collect()),
                    _ => Val::Array(items.iter().zip(&truths).filter(|(_, t)| !**t).map(|(i, _)| i.clone()).collect()),
                })
            }
            ("[]", Val::Hash(entries), [key]) => Ok(entries.iter().find(|(k, _)| same(k, key)).map(|(_, v)| v.clone()).unwrap_or(Val::Nil)),
            ("[]", Val::Array(items), [Val::Int(i)]) => Ok(usize::try_from(*i).ok().and_then(|i| items.get(i)).cloned().unwrap_or(Val::Nil)),
            ("==", _, [other]) => Ok(Val::Bool(same(&this, other))),
            ("!=", _, [other]) => Ok(Val::Bool(!same(&this, other))),
            ("<" | ">" | "<=" | ">=", Val::Int(a), [Val::Int(b)]) => Ok(Val::Bool(match method {
                "<" => a < b,
                ">" => a > b,
                "<=" => a <= b,
                _ => a >= b,
            })),
            ("+" | "-" | "*", Val::Int(a), [Val::Int(b)]) => {
                let v = match method {
                    "+" => a.checked_add(*b),
                    "-" => a.checked_sub(*b),
                    _ => a.checked_mul(*b),
                };
                v.map(Val::Int).ok_or_else(|| "an Integer past 64 bits".into())
            }
            // A built-in on a value that is not the program's: it runs no method of the program and writes none of its ivars.
            _ if blk.is_none()
                && !matches!(this, Val::Obj(_) | Val::Unknown)
                && !matches!(&this, Val::Class(c) if self.known.contains(c))
                && !argv.iter().any(|a| matches!(a, Val::Proc { .. } | Val::Obj(_)))
                && !mutates(method) =>
            {
                Ok(Val::Unknown)
            }
            _ => Err(format!("it cannot call `{method}`")),
        }
    }

    fn call_proc(&mut self, proc_: &Val, argv: Vec<Val>, frame: &Frame) -> Res<Val> {
        let Val::Proc { params, rest, body, scope } = proc_ else { return Err("a block that is not a block".into()) };
        let mut inner = Frame { this: frame.this.clone(), locals: frame.locals.clone(), block: None, scope: scope.clone() };
        bind(&mut inner.locals, params, rest.as_ref(), argv);
        self.value(body, &mut inner)
    }

    fn invoke(&mut self, def: &MethodDef, scope: String, this: Val, argv: Vec<Val>, blk: Option<Val>) -> Res<Val> {
        if self.depth > MAX_DEPTH {
            return Err("its calls nest too deep".into());
        }
        let mut frame = Frame { this, locals: HashMap::default(), block: blk.clone(), scope: Some(scope) };
        let takes_keywords = def.params.iter().any(|p| p.keyword || p.from_keyword);
        let mut keywords: Vec<(Val, Val)> = Vec::new();
        let mut argv = argv;
        if takes_keywords && matches!(argv.last(), Some(Val::Hash(_))) {
            if let Some(Val::Hash(entries)) = argv.pop() {
                keywords = entries;
            }
        }
        let mut given = argv.into_iter();
        for p in &def.params {
            if p.from_kwrest {
                return Err("a keyword rest parameter".into());
            }
            if p.keyword || p.from_keyword {
                let found = keywords.iter().position(|(k, _)| matches!(k, Val::Sym(n) if n == p.name.as_str()));
                let v = match (found, &p.default) {
                    (Some(i), _) => keywords.remove(i).1,
                    (None, Some(d)) => {
                        let d = d.clone();
                        self.value(&d, &mut frame)?
                    }
                    (None, None) => return Err(format!("`{}:` is not given", p.name.as_str())),
                };
                frame.locals.insert(p.name.clone(), v);
                continue;
            }
            if p.rest {
                frame.locals.insert(p.name.clone(), Val::Array(given.by_ref().collect()));
                continue;
            }
            let v = match (given.next(), &p.default) {
                (Some(v), _) => v,
                (None, Some(d)) => {
                    let d = d.clone();
                    self.value(&d, &mut frame)?
                }
                (None, None) => return Err(format!("`{}` is not given", p.name.as_str())),
            };
            frame.locals.insert(p.name.clone(), v);
        }
        if let Some(b) = &def.block_param {
            frame.locals.insert(b.name.clone(), blk.unwrap_or(Val::Nil));
        }
        self.depth += 1;
        let ran = self.value(&def.body, &mut frame);
        self.depth -= 1;
        ran
    }

    /// The method `name` answers on `this`, and the class its body's constants resolve in.
    fn lookup(&self, this: &Val, name: &str) -> Option<(MethodDef, String)> {
        let (class, receiver) = match this {
            Val::Obj(i) => (self.heap[*i].class.clone(), MethodReceiver::Instance),
            Val::Class(c) if self.known.contains(c) => (c.clone(), MethodReceiver::Class),
            _ => return None,
        };
        let mut seen = HashSet::default();
        self.lookup_in(&class, name, receiver, &mut seen)
    }

    fn lookup_in(&self, class: &str, name: &str, receiver: MethodReceiver, seen: &mut HashSet<String>) -> Option<(MethodDef, String)> {
        if !seen.insert(class.to_string()) {
            return None;
        }
        let decls: Vec<&LibraryClass> = self.classes.iter().filter(|c| c.name.0.as_str() == class).collect();
        for c in decls.iter().rev() {
            if let Some(m) = c.methods.iter().rev().find(|m| m.name.as_str() == name && m.receiver == receiver) {
                let scope = m.enclosing_class.as_ref().map(|s| s.as_str().to_string()).unwrap_or_else(|| class.to_string());
                return Some((m.clone(), scope));
            }
        }
        // Not `extend`ed modules on the class side: what reaches a class there is its own `def self.`s and its ancestors'.
        if receiver == MethodReceiver::Instance {
            for c in &decls {
                for inc in c.includes.iter().rev() {
                    if let Some(full) = self.resolve(inc.0.as_str(), Some(class))
                        && let Some(found) = self.lookup_in(&full, name, MethodReceiver::Instance, seen)
                    {
                        return Some(found);
                    }
                }
            }
        }
        let parent = decls.iter().find_map(|c| c.parent.clone())?;
        let parent = self.resolve(parent.0.as_str(), Some(class))?;
        self.lookup_in(&parent, name, receiver, seen)
    }

    fn resolve(&self, written: &str, scope: Option<&str>) -> Option<String> {
        let rooted = written.starts_with("::");
        let written = written.trim_start_matches("::");
        let mut at = if rooted { None } else { scope.map(str::to_string) };
        while let Some(outer) = at {
            let candidate = format!("{outer}::{written}");
            if self.known.contains(&candidate) {
                return Some(candidate);
            }
            at = outer.rsplit_once("::").map(|(o, _)| o.to_string());
        }
        self.known.contains(written).then(|| written.to_string())
    }

    /// A constant: a class this program defines, a constant one of them assigns, or a name it does not define.
    fn constant(&mut self, path: &[Symbol], scope: Option<&str>) -> Res<Val> {
        let written = path.iter().map(|s| s.as_str()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join("::");
        let written = if path.first().is_some_and(|s| s.as_str().is_empty()) { format!("::{written}") } else { written };
        if let Some(full) = self.resolve(&written, scope) {
            return Ok(Val::Class(full));
        }
        let (owner, last) = match written.trim_start_matches("::").rsplit_once("::") {
            Some((o, l)) => (self.resolve(o, scope), l.to_string()),
            None => (None, written.trim_start_matches("::").to_string()),
        };
        let mut at = owner.or_else(|| scope.map(str::to_string));
        while let Some(class) = at {
            let value = self
                .classes
                .iter()
                .filter(|c| c.name.0.as_str() == class)
                .flat_map(|c| c.constants.iter())
                .find(|(n, _)| n.as_str() == last)
                .map(|(_, v)| v.clone());
            if let Some(value) = value {
                let mut frame = Frame { this: Val::Class(class.clone()), locals: HashMap::default(), block: None, scope: Some(class.clone()) };
                return self.value(&value, &mut frame);
            }
            at = class.rsplit_once("::").map(|(o, _)| o.to_string());
        }
        Ok(Val::Class(written.trim_start_matches("::").to_string()))
    }

    fn ivar(&self, this: &Val, name: &str) -> Option<Val> {
        match this {
            Val::Obj(i) => self.heap[*i].ivars.get(name).cloned(),
            Val::Class(c) => self.class_ivars.get(&(c.clone(), name.to_string())).cloned(),
            _ => None,
        }
    }

    fn set_ivar(&mut self, this: &Val, name: &str, v: Val) -> Res<()> {
        if let Some(h) = Self::holder(this) {
            self.tick += 1;
            self.written.insert((h, name.to_string()), self.tick);
        }
        match this {
            Val::Obj(i) => {
                self.heap[*i].ivars.insert(name.to_string(), v);
            }
            Val::Class(c) => {
                self.class_ivars.insert((c.clone(), name.to_string()), v);
            }
            _ => return Err("an instance variable of other than an object".into()),
        }
        Ok(())
    }

    fn text(&self, v: &Val) -> Res<String> {
        match v {
            Val::Str(s) | Val::Sym(s) | Val::Class(s) => Ok(s.clone()),
            Val::Int(i) => Ok(i.to_string()),
            Val::Nil => Ok(String::new()),
            Val::Bool(b) => Ok(b.to_string()),
            _ => Err("text of other than a literal".into()),
        }
    }

    // A class with `method_missing` may answer any name with any of its methods.
    fn answers_missing(&self, class: &str) -> bool {
        self.ancestry(class).iter().any(|a| {
            self.classes.iter().any(|c| c.name.0.as_str() == a && c.methods.iter().any(|m| m.name.as_str() == "method_missing"))
        })
    }

    fn owner_tag(&self, c: &LibraryClass, m: &MethodDef) -> (String, bool) {
        // Not the module's own: an included or extended module's method writes the ivars of whoever it runs on.
        if c.is_module && m.receiver == MethodReceiver::Instance { ("*".into(), false) } else { (c.name.0.as_str().to_string(), m.receiver == MethodReceiver::Class) }
    }

    fn all_method_writes(&self) -> Option<HashSet<Tag>> {
        let mut out = HashSet::default();
        for c in self.classes {
            for m in &c.methods {
                let (owner, side) = self.owner_tag(c, m);
                if !self.direct_writes(&m.body, &owner, side, &mut out) {
                    return None;
                }
            }
        }
        Some(out)
    }

    /// The writes `e` makes itself, without the methods it calls; `false` for one whose name is known only when it runs.
    fn direct_writes(&self, e: &Expr, owner: &str, side: bool, out: &mut HashSet<Tag>) -> bool {
        let bounded = true;
        visit(e, &mut |x| match &*x.node {
            ExprNode::Assign { target: LValue::Ivar { name }, .. } | ExprNode::OpAssign { target: LValue::Ivar { name }, .. } => {
                out.insert((owner.to_string(), side, name.as_str().to_string()));
            }
            // `@scopes << :read`: the object the ivar holds changes, which a read of the ivar sees; a program's own method is followed instead.
            ExprNode::Send { recv: Some(r), method, .. }
                if let ExprNode::Ivar { name } = &*r.node
                    && !self.classes.iter().any(|c| c.methods.iter().any(|m| &m.name == method))
                    && (method.as_str().ends_with('!')
                        || matches!(method.as_str(), "<<" | "push" | "[]=" | "concat" | "update" | "delete" | "clear" | "replace" | "insert" | "unshift" | "store" | "append" | "prepend")) =>
            {
                out.insert((owner.to_string(), side, name.as_str().to_string()));
            }
            ExprNode::Send { recv, method, args, .. } if method.as_str() == "instance_variable_set" => match args.first().map(|a| &*a.node) {
                Some(ExprNode::Lit { value: Literal::Sym { value } }) => {
                    let name = value.as_str().trim_start_matches('@').to_string();
                    match recv.as_ref().map(|r| &*r.node) {
                        None | Some(ExprNode::SelfRef) => out.insert((owner.to_string(), side, name)),
                        Some(_) => out.insert(("*".into(), false, name)),
                    };
                }
                // A name known only when it runs: the names its literal parts allow.
                arg => {
                    let pattern = arg.map(name_pattern).unwrap_or_else(|| "*".into());
                    match recv.as_ref().map(|r| &*r.node) {
                        None | Some(ExprNode::SelfRef) => out.insert((owner.to_string(), side, pattern)),
                        Some(_) => out.insert(("*".into(), false, pattern)),
                    };
                }
            },
            _ => {}
        });
        bounded
    }

    /// The instance variables a statement may write, through every method of the program it may call: `None` when unbounded.
    fn may_write(&self, e: &Expr, owner: &str, side: bool, visited: &mut HashSet<String>) -> Option<HashSet<Tag>> {
        let mut out = HashSet::default();
        // Not by recursion: a large program's call chains run deeper than a stack bound, and each method is read once.
        let mut work: Vec<(Expr, String, bool, Option<String>)> = vec![(e.clone(), owner.to_string(), side, (owner != "*").then(|| owner.to_string()))];
        while let Some((body, owner, side, class)) = work.pop() {
            if !self.direct_writes(&body, &owner, side, &mut out) {
                return None;
            }
            let mut calls: Vec<(String, Option<Expr>, bool)> = Vec::new();
            visit(&body, &mut |x| {
                if let ExprNode::Send { recv, method, args, .. } = &*x.node {
                    let dynamic = matches!(method.as_str(), "send" | "public_send" | "__send__" | "class_eval" | "define_method")
                        && !matches!(args.first().map(|a| &*a.node), Some(ExprNode::Lit { value: Literal::Sym { .. } }));
                    calls.push((method.as_str().to_string(), recv.clone(), dynamic));
                }
            });
            for (name, recv, dynamic) in calls {
                if dynamic {
                    // On self, a name known only when it runs is still one of the class's own methods.
                    let own = matches!(recv.as_ref().map(|r| &*r.node), None | Some(ExprNode::SelfRef)).then_some(class.as_ref()).flatten();
                    match own {
                        Some(c) => {
                            let receiver = if side { MethodReceiver::Class } else { MethodReceiver::Instance };
                            for ancestor in self.ancestry(c) {
                                for decl in self.classes.iter().filter(|d| d.name.0.as_str() == ancestor) {
                                    for m in decl.methods.iter().filter(|m| m.receiver == receiver || decl.is_module) {
                                        let key = format!("{}#{}#{:?}#{:?}", decl.name.0.as_str(), m.name.as_str(), m.receiver, m.body.span);
                                        if visited.insert(key) {
                                            let (o, s) = self.owner_tag(decl, m);
                                            work.push((m.body.clone(), o, s, Some(ancestor.clone())));
                                        }
                                    }
                                }
                            }
                        }
                        None => {
                            out.extend(self.all_method_writes()?)
                        }
                    }
                    continue;
                }
                // The class the call is on, when the receiver says: then its own lookup answers, and no method there is a built-in.
                let on: Option<(String, MethodReceiver)> = match recv.as_ref().map(|r| &*r.node) {
                    None | Some(ExprNode::SelfRef) => {
                        class.clone().map(|c| (c, if side { MethodReceiver::Class } else { MethodReceiver::Instance }))
                    }
                    Some(ExprNode::Const { path }) => {
                        let written = path.iter().map(|s| s.as_str()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join("::");
                        let resolved = self.resolve(&written, class.as_deref());
                        // Not a class the program defines: a framework's, whose methods write none of its ivars.
                        let Some(resolved) = resolved else { continue };
                        Some((resolved, MethodReceiver::Class))
                    }
                    _ => None,
                };
                let exact = match &on {
                    Some((c, MethodReceiver::Class)) if name == "new" => self.lookup_in(c, "initialize", MethodReceiver::Instance, &mut HashSet::default()),
                    Some((c, receiver)) => self.lookup_in(c, &name, *receiver, &mut HashSet::default()),
                    None => None,
                };
                if on.is_some() && exact.is_none() && !self.answers_missing(on.as_ref().map(|(c, _)| c.as_str()).unwrap_or("")) {
                    continue;
                }
                let callees: Vec<(&LibraryClass, &MethodDef)> = match exact {
                    Some((def, _)) => self
                        .classes
                        .iter()
                        .flat_map(|c| c.methods.iter().map(move |m| (c, m)))
                        .filter(|(_, m)| m.name == def.name && m.receiver == def.receiver && m.body.span == def.body.span)
                        .take(1)
                        .collect(),
                    // Every method of the program by that name: which one the call reaches is not known here.
                    None => self.classes.iter().flat_map(|c| c.methods.iter().filter(|m| m.name.as_str() == name).map(move |m| (c, m))).collect(),
                };
                for (c, m) in callees {
                    let key = format!("{}#{}#{:?}#{:?}", c.name.0.as_str(), m.name.as_str(), m.receiver, m.body.span);
                    if visited.insert(key) {
                        let (o, s) = self.owner_tag(c, m);
                        let at = (!c.is_module || m.receiver == MethodReceiver::Class).then(|| c.name.0.as_str().to_string());
                        work.push((m.body.clone(), o, s, at));
                    }
                }
            }
        }
        Some(out)
    }
}

fn visit(e: &Expr, f: &mut dyn FnMut(&Expr)) {
    f(e);
    e.node.for_each_child(&mut |c| visit(c, f));
}

fn bind(locals: &mut HashMap<Symbol, Val>, params: &[Symbol], rest: Option<&Symbol>, argv: Vec<Val>) {
    let mut given = argv.into_iter();
    for p in params {
        locals.insert(p.clone(), given.next().unwrap_or(Val::Nil));
    }
    if let Some(r) = rest {
        locals.insert(r.clone(), Val::Array(given.collect()));
    }
}

fn literal(value: &Literal) -> Res<Val> {
    Ok(match value {
        Literal::Nil => Val::Nil,
        Literal::Bool { value } => Val::Bool(*value),
        Literal::Int { value } => Val::Int(*value),
        Literal::Str { value } => Val::Str(value.clone()),
        Literal::Sym { value } => Val::Sym(value.as_str().to_string()),
        // Ruby's `^` and `$` are line anchors; flags other than those it can carry are not read here.
        Literal::Regex { pattern, flags } if flags.chars().all(|f| matches!(f, 'i' | 'm' | 'x')) => {
            let mut inline = String::from("m");
            if flags.contains('i') {
                inline.push('i');
            }
            if flags.contains('x') {
                inline.push('x');
            }
            if flags.contains('m') {
                inline.push('s');
            }
            Val::Regex(format!("(?{inline}){pattern}"))
        }
        _ => return Err("a literal of another kind".into()),
    })
}

fn same(a: &Val, b: &Val) -> bool {
    match (a, b) {
        (Val::Nil, Val::Nil) => true,
        (Val::Bool(x), Val::Bool(y)) => x == y,
        (Val::Int(x), Val::Int(y)) => x == y,
        (Val::Str(x), Val::Str(y)) | (Val::Sym(x), Val::Sym(y)) | (Val::Class(x), Val::Class(y)) => x == y,
        (Val::Obj(x), Val::Obj(y)) => x == y,
        _ => false,
    }
}

/// The constant a call chain starts from: `Doorkeeper` in `Doorkeeper.config.resolve_controller(:base)`.
fn root_constant(e: &Expr) -> Option<String> {
    match &*e.node {
        ExprNode::Send { recv: Some(r), .. } => root_constant(r),
        ExprNode::Const { path } => Some(path.iter().map(|s| s.as_str()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join("::")),
        _ => None,
    }
}

// Not answered as `Unknown`: a built-in that changes its receiver changes a value the program may read back.
fn mutates(method: &str) -> bool {
    method.ends_with('!')
        || matches!(method, "<<" | "push" | "[]=" | "concat" | "update" | "delete" | "clear" | "replace" | "insert" | "unshift" | "store" | "append" | "prepend" | "send" | "public_send" | "__send__" | "instance_variable_set" | "instance_eval" | "instance_exec" | "class_eval" | "define_method" | "extend" | "include")
}

/// The ivar names an `instance_variable_set` name expression can spell, as a pattern (`@#{type}_secret_strategy` -> `*_secret_strategy`).
fn name_pattern(node: &ExprNode) -> String {
    let parts = match node {
        ExprNode::Send { recv: Some(r), method, .. } if method.as_str() == "to_sym" => match &*r.node {
            ExprNode::StringInterp { parts } => parts,
            _ => return "*".into(),
        },
        ExprNode::StringInterp { parts } => parts,
        ExprNode::Lit { value: Literal::Str { value } } => return value.trim_start_matches('@').to_string(),
        _ => return "*".into(),
    };
    let mut out = String::new();
    for part in parts {
        match part {
            InterpPart::Text { value } => out.push_str(value),
            InterpPart::Expr { .. } => {
                if !out.ends_with('*') {
                    out.push('*');
                }
            }
        }
    }
    let out = out.trim_start_matches('@').to_string();
    if out.is_empty() { "*".into() } else { out }
}

fn glob(pattern: &str, name: &str) -> bool {
    let pieces: Vec<&str> = pattern.split('*').collect();
    if pieces.len() == 1 {
        return pattern == name;
    }
    let (first, last) = (pieces[0], pieces[pieces.len() - 1]);
    if !name.starts_with(first) || !name[first.len()..].ends_with(last) || name.len() < first.len() + last.len() {
        return false;
    }
    let mut rest = &name[first.len()..name.len() - last.len()];
    for piece in &pieces[1..pieces.len() - 1] {
        match rest.find(piece) {
            Some(i) => rest = &rest[i + piece.len()..],
            None => return false,
        }
    }
    true
}
