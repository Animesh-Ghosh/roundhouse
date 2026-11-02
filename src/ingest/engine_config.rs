//! An engine's config namespace copied onto a module at boot, as
//! mission_control-jobs 1.1.0's engine does:
//!
//! ```ruby
//! config.before_initialize do
//!   config.mission_control.jobs.each do |key, value|
//!     MissionControl::Jobs.public_send("#{key}=", value)
//!   end
//! end
//! ```
//!
//! The host sets the namespace in its `Rails::Application` body
//! (`config.mission_control.jobs.base_controller_class = "JobsModController"`).
//! Each such literal write is the module's setter at boot: it joins the
//! boot program ahead of the host's initializers, as `before_initialize`
//! runs before `load_config_initializers`, and leaves the ledger.
//!
//! The value is fixed before any class loads, so a superclass read from
//! it (`class ApplicationController < MissionControl::Jobs.base_controller_class.constantize`)
//! names a constant: the host's string, else the module's
//! `mattr_accessor …, default: "<name>"` ([`folded_superclass`]). A key the
//! source mentions anywhere else (an environment file, an initializer, the
//! engine itself, a conditional) has no single value, and folds nothing.
//!
//! The engine's `before_initialize` blocks are also run here, over what is
//! known by then: the modules' `mattr` defaults, the host's config and its
//! `config.active_job.queue_adapter` (`Boot`). A branch whose condition
//! that settles is one boot never takes ([`dead_spans`]): mission_control-jobs
//! prepends its Resque extension only `if MissionControl::Jobs.adapters.include?(:resque)`,
//! after filling the adapters from the queue adapter. A condition it
//! cannot settle keeps both branches. Nothing of the blocks is replayed
//! yet; the dead branches only stop counting as a use
//! (`unreached_gem_modules`).

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use ruby_prism::Node;

use crate::app::App;
use crate::det::{HashMap, HashSet};
use crate::expr::{Expr, ExprNode, LValue};
use crate::ident::Symbol;
use crate::span::Span;
use crate::vfs::Vfs;

use super::prism_api::PrismLocation;
use super::util::{constant_id_str, constant_path_of, find_all_classes_with_scope, flatten_statements};

const APPLICATION: &str = "config/application.rb";

/// `config.<path>.each { |k, v| <module>.public_send("#{k}=", v) }` in an engine.
#[derive(Clone, Debug)]
struct Forwarder {
    path: Vec<String>,
    module: String,
    lib: PathBuf,
}

/// `config.<path>.<attr> = <value>` in the host's application body.
#[derive(Clone, Debug)]
struct Setting {
    module: String,
    attr: String,
    value: Expr,
    string: Option<String>,
    start: u32,
}

#[derive(Default)]
struct State {
    settings: Vec<Setting>,
    superclasses: HashMap<(String, String), String>,
    dead: Vec<(String, u32, u32)>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Reads the forwarders under `libs` and the host's writes to them; the
/// rest of this file answers from what it found.
pub(super) fn prepare<V: Vfs + ?Sized>(vfs: &V, dir: &Path, libs: &[PathBuf]) {
    let mut state = State::default();
    let forwarders = forwarders(vfs, libs);
    if !forwarders.is_empty() {
        state.settings = host_settings(vfs, dir, &forwarders);
        state.superclasses = superclasses(vfs, dir, &forwarders, &state.settings);
    }
    state.dead = dead_branches(vfs, dir, libs, &forwarders);
    STATE.with(|s| *s.borrow_mut() = state);
}

/// The constant a `<module>.<attr>.constantize` superclass names at load.
pub(super) fn folded_superclass(node: &Node<'_>) -> Option<Vec<String>> {
    let call = node.as_call_node()?;
    if constant_id_str(&call.name()) != "constantize" || call.arguments().is_some() || call.block().is_some() {
        return None;
    }
    let reader = call.receiver()?.as_call_node()?;
    if reader.arguments().is_some() || reader.block().is_some() {
        return None;
    }
    let module = constant_path_of(&reader.receiver()?)?.join("::");
    let attr = constant_id_str(&reader.name()).to_string();
    let name = STATE.with(|s| s.borrow().superclasses.get(&(module, attr)).cloned())?;
    Some(name.trim_start_matches("::").split("::").map(str::to_string).collect())
}

/// The branches of the engines' `before_initialize` that boot does not take, by file path.
pub(super) fn dead_spans() -> Vec<(String, u32, u32)> {
    STATE.with(|s| s.borrow().dead.clone())
}

/// Puts the host's settings at the head of the boot program; returns the
/// ledger keys of the statements it carried.
pub(super) fn carry(app: &mut App, dir: &Path) -> super::boot_program::Taken {
    let settings = STATE.with(|s| s.borrow().settings.clone());
    let file = super::sources::file_id(&dir.join(APPLICATION).display().to_string());
    let mut taken = super::boot_program::Taken::default();
    let mut statements = Vec::new();
    for setting in settings {
        let span = Span { file, start: setting.start, end: setting.value.span.end };
        let mut recv = Expr::new(span, ExprNode::Const { path: setting.module.split("::").map(Symbol::from).collect() });
        // Not resolved from the source at its span: that span is the host's `config.…` write, which never names the module.
        recv.decisions |= crate::expr::GENERATED_CONST_REF;
        let target = LValue::Attr { recv, name: Symbol::from(setting.attr.as_str()) };
        statements.push(Expr::new(span, ExprNode::Assign { target, value: setting.value }));
        taken.insert((APPLICATION.to_string(), setting.start));
    }
    app.boot_program.splice(0..0, statements);
    taken
}

fn forwarders<V: Vfs + ?Sized>(vfs: &V, libs: &[PathBuf]) -> Vec<Forwarder> {
    let mut out = Vec::new();
    for lib in libs {
        if !vfs.is_dir(lib) {
            continue;
        }
        let Ok(files) = super::app::read_rb_files(vfs, lib) else { continue };
        for file in files {
            let Ok(source) = vfs.read(&file) else { continue };
            if !contains(&source, b"Engine") || !contains(&source, b"send(\"#{") {
                continue;
            }
            let parsed = super::prism::parse_silent(&source);
            for (_, class) in find_all_classes_with_scope(&parsed.node()) {
                let is_engine = class
                    .superclass()
                    .and_then(|s| constant_path_of(&s))
                    .is_some_and(|p| p == ["Rails", "Engine"]);
                let Some(body) = class.body().filter(|_| is_engine) else { continue };
                for stmt in flatten_statements(body) {
                    let Some(hook) = config_hook(&stmt, "before_initialize") else { continue };
                    for inner in hook {
                        if let Some((path, module)) = forwarder(&inner) {
                            out.push(Forwarder { path, module, lib: lib.clone() });
                        }
                    }
                }
            }
        }
    }
    out
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// The statements of `config.<hook> do … end`.
fn config_hook<'pr>(node: &Node<'pr>, hook: &str) -> Option<Vec<Node<'pr>>> {
    let call = node.as_call_node()?;
    if constant_id_str(&call.name()) != hook || config_chain(&call.receiver()?)? != Vec::<String>::new() {
        return None;
    }
    let block = call.block()?.as_block_node()?;
    Some(block.body().map(|b| flatten_statements(b)).unwrap_or_default())
}

/// `[]` for `config`, `["a", "b"]` for `config.a.b`.
fn config_chain(node: &Node<'_>) -> Option<Vec<String>> {
    let call = node.as_call_node()?;
    if call.arguments().is_some() || call.block().is_some() {
        return None;
    }
    let name = constant_id_str(&call.name()).to_string();
    match call.receiver() {
        None => (name == "config").then(Vec::new),
        Some(receiver) => {
            let mut path = config_chain(&receiver)?;
            path.push(name);
            Some(path)
        }
    }
}

fn forwarder(node: &Node<'_>) -> Option<(Vec<String>, String)> {
    let call = node.as_call_node()?;
    if constant_id_str(&call.name()) != "each" || call.arguments().is_some() {
        return None;
    }
    let path = config_chain(&call.receiver()?).filter(|p| !p.is_empty())?;
    let block = call.block()?.as_block_node()?;
    let params = block.parameters()?.as_block_parameters_node()?.parameters()?;
    let names: Vec<String> = params
        .requireds()
        .iter()
        .filter_map(|p| p.as_required_parameter_node().map(|p| constant_id_str(&p.name()).to_string()))
        .collect();
    let [key, value] = names.as_slice() else { return None };
    let [send] = flatten_statements(block.body()?).try_into().ok()?;
    let send = send.as_call_node()?;
    if !matches!(constant_id_str(&send.name()), "public_send" | "send") {
        return None;
    }
    let module = constant_path_of(&send.receiver()?)?.join("::");
    let args: Vec<Node<'_>> = send.arguments()?.arguments().iter().collect();
    let [setter, passed] = args.as_slice() else { return None };
    let is_local = |n: &Node<'_>, name: &str| {
        n.as_local_variable_read_node().is_some_and(|r| constant_id_str(&r.name()) == name)
    };
    if !is_local(passed, value) {
        return None;
    }
    let parts: Vec<Node<'_>> = setter.as_interpolated_string_node()?.parts().iter().collect();
    let [embedded, suffix] = parts.as_slice() else { return None };
    let stmts: Vec<Node<'_>> = embedded.as_embedded_statements_node()?.statements()?.body().iter().collect();
    let [only] = stmts.as_slice() else { return None };
    if !is_local(only, key) || suffix.as_string_node()?.unescaped() != b"=" {
        return None;
    }
    Some((path, module))
}

fn host_settings<V: Vfs + ?Sized>(vfs: &V, dir: &Path, forwarders: &[Forwarder]) -> Vec<Setting> {
    let file = dir.join(APPLICATION);
    let Ok(source) = vfs.read(&file) else { return Vec::new() };
    let file_name = file.display().to_string();
    let parsed = super::prism::parse_silent(&source);
    let mut out = Vec::new();
    for (_, class) in find_all_classes_with_scope(&parsed.node()) {
        let is_application = class
            .superclass()
            .and_then(|s| constant_path_of(&s))
            .is_some_and(|p| p == ["Rails", "Application"]);
        let Some(body) = class.body().filter(|_| is_application) else { continue };
        for stmt in flatten_statements(body) {
            let Some(call) = stmt.as_call_node() else { continue };
            let name = constant_id_str(&call.name());
            let Some(attr) = name.strip_suffix('=').filter(|a| !a.is_empty() && !a.ends_with(['=', '!', '<', '>'])) else {
                continue;
            };
            let Some(path) = call.receiver().and_then(|r| config_chain(&r)) else { continue };
            let Some(fw) = forwarders.iter().find(|f| f.path == path) else { continue };
            let args: Vec<Node<'_>> = call.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default();
            let [value] = args.as_slice() else { continue };
            if !literal(value) {
                continue;
            }
            let (read, _) = super::prism::scope(|| super::expr::ingest_expr(value, &file_name));
            let Ok(expr) = read else { continue };
            let string = value.as_string_node().map(|s| String::from_utf8_lossy(s.unescaped()).into_owned());
            out.push(Setting {
                module: fw.module.clone(),
                attr: attr.to_string(),
                value: expr,
                string,
                start: stmt.location().start_offset() as u32,
            });
        }
    }
    out
}

// Not any expression: the write moves ahead of the initializers, where a constant or call it names may not be defined yet.
fn literal(node: &Node<'_>) -> bool {
    node.as_string_node().is_some()
        || node.as_symbol_node().is_some()
        || node.as_true_node().is_some()
        || node.as_false_node().is_some()
        || node.as_nil_node().is_some()
        || node.as_integer_node().is_some()
        || node.as_float_node().is_some()
}

/// `(module, attr)` to the class name its value is when classes load.
fn superclasses<V: Vfs + ?Sized>(
    vfs: &V,
    dir: &Path,
    forwarders: &[Forwarder],
    settings: &[Setting],
) -> HashMap<(String, String), String> {
    let mut sources: Vec<Vec<u8>> = Vec::new();
    for sub in ["config/environments", "config/initializers"] {
        let root = dir.join(sub);
        if vfs.is_dir(&root) {
            for file in super::app::read_rb_files(vfs, &root).unwrap_or_default() {
                sources.extend(vfs.read(&file).ok());
            }
        }
    }
    let application = vfs.read(&dir.join(APPLICATION)).unwrap_or_default();
    let mut out = HashMap::default();
    let mut seen: HashSet<(String, PathBuf)> = HashSet::default();
    for fw in forwarders {
        if !seen.insert((fw.module.clone(), fw.lib.clone())) {
            continue;
        }
        let lib_files = super::app::read_rb_files(vfs, &fw.lib).unwrap_or_default();
        let lib_sources: Vec<Vec<u8>> = lib_files.iter().filter_map(|f| vfs.read(f).ok()).collect();
        let defaults = string_defaults(&lib_sources, &fw.module);
        let mut attrs: Vec<String> = defaults.iter().map(|(a, _)| a.clone()).collect();
        attrs.extend(settings.iter().filter(|s| s.module == fw.module).map(|s| s.attr.clone()));
        attrs.sort();
        attrs.dedup();
        for attr in attrs {
            let written: Vec<&Setting> = settings.iter().filter(|s| s.module == fw.module && s.attr == attr).collect();
            let config_key = format!("{}.{attr}", fw.path.join("."));
            let module_key = format!("{}.{attr}", fw.module);
            let elsewhere = sources.iter().chain(lib_sources.iter()).any(|t| mentions(t, &config_key) + mentions(t, &module_key) > 0)
                || mentions(&application, &module_key) > 0
                || mentions(&application, &config_key) != written.len();
            let value = match written.as_slice() {
                [] => defaults.iter().find(|(a, _)| *a == attr).map(|(_, d)| d.clone()),
                [only] => only.string.clone(),
                _ => None,
            };
            if let Some(value) = value.filter(|_| !elsewhere) {
                out.insert((fw.module.clone(), attr), value);
            }
        }
    }
    out
}

/// Occurrences of `key` not followed by an identifier character.
fn mentions(text: &[u8], key: &str) -> usize {
    let key = key.as_bytes();
    text.windows(key.len())
        .enumerate()
        .filter(|(i, w)| {
            *w == key
                && !text.get(i + key.len()).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
                && !(*i > 0 && (text[i - 1].is_ascii_alphanumeric() || text[i - 1] == b'_' || text[i - 1] == b':'))
        })
        .count()
}

/// `mattr_accessor :attr, default: "<string>"` directly in `module`'s body.
fn string_defaults(sources: &[Vec<u8>], module: &str) -> Vec<(String, String)> {
    defaults(sources, module)
        .into_iter()
        .filter_map(|(attr, value)| match value {
            Val::Str(s) => Some((attr, s)),
            _ => None,
        })
        .collect()
}

/// `mattr_accessor :attr, default: <value>` directly in `module`'s body.
fn defaults(sources: &[Vec<u8>], module: &str) -> Vec<(String, Val)> {
    let mut out = Vec::new();
    for source in sources {
        let parsed = super::prism::parse_silent(source);
        let mut stack = Vec::new();
        modules(&parsed.node(), &mut stack, module, &mut out);
    }
    out
}

fn modules(node: &Node<'_>, stack: &mut Vec<String>, wanted: &str, out: &mut Vec<(String, Val)>) {
    if let Some(program) = node.as_program_node() {
        for stmt in program.statements().body().iter() {
            modules(&stmt, stack, wanted, out);
        }
        return;
    }
    let Some(m) = node.as_module_node() else { return };
    let Some(name) = constant_path_of(&m.constant_path()) else { return };
    let depth = stack.len();
    stack.extend(name);
    if let Some(body) = m.body() {
        let here = stack.join("::") == wanted;
        for stmt in flatten_statements(body) {
            if here {
                out.extend(mattr_default(&stmt));
            }
            modules(&stmt, stack, wanted, out);
        }
    }
    stack.truncate(depth);
}

fn mattr_default(node: &Node<'_>) -> Vec<(String, Val)> {
    let Some(call) = node.as_call_node() else { return Vec::new() };
    if call.receiver().is_some() || !matches!(constant_id_str(&call.name()), "mattr_accessor" | "cattr_accessor" | "mattr_reader" | "cattr_reader") {
        return Vec::new();
    }
    let args: Vec<Node<'_>> = call.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default();
    let mut names = Vec::new();
    let mut default = Val::Nil;
    for arg in &args {
        if let Some(sym) = arg.as_symbol_node() {
            names.push(String::from_utf8_lossy(sym.unescaped()).into_owned());
        } else if let Some(hash) = arg.as_keyword_hash_node() {
            for element in hash.elements().iter() {
                let Some(pair) = element.as_assoc_node() else { continue };
                if pair.key().as_symbol_node().is_some_and(|k| k.unescaped() == b"default") {
                    default = literal_val(&pair.value());
                }
            }
        }
    }
    names.into_iter().map(|n| (n, default.clone())).collect()
}

/// What the evaluation of an engine's `before_initialize` knows of a value.
#[derive(Clone, Debug, PartialEq)]
enum Val {
    Nil,
    Bool(bool),
    Int(i64),
    Str(String),
    Sym(String),
    /// A `Set` whose every member is known.
    Set(Vec<Val>),
    Unknown,
}

impl Val {
    fn truthy(&self) -> Option<bool> {
        match self {
            Val::Nil | Val::Bool(false) => Some(false),
            Val::Unknown => None,
            _ => Some(true),
        }
    }
}

fn literal_val(node: &Node<'_>) -> Val {
    if let Some(s) = node.as_string_node() {
        return Val::Str(String::from_utf8_lossy(s.unescaped()).into_owned());
    }
    if let Some(s) = node.as_symbol_node() {
        return Val::Sym(String::from_utf8_lossy(s.unescaped()).into_owned());
    }
    if node.as_true_node().is_some() {
        return Val::Bool(true);
    }
    if node.as_false_node().is_some() {
        return Val::Bool(false);
    }
    if node.as_nil_node().is_some() {
        return Val::Nil;
    }
    if let Some(i) = node.as_integer_node() {
        return String::from_utf8_lossy(i.location().as_slice()).replace('_', "").parse().map(Val::Int).unwrap_or(Val::Unknown);
    }
    // `Set.new` with nothing in it: the empty Set mission_control-jobs starts its adapters from.
    if let Some(call) = node.as_call_node()
        && constant_id_str(&call.name()) == "new"
        && call.arguments().is_none()
        && call.block().is_none()
        && call.receiver().and_then(|r| constant_path_of(&r)).is_some_and(|p| p == ["Set"])
    {
        return Val::Set(Vec::new());
    }
    Val::Unknown
}

/// `before_initialize` run with what is known at that point of boot: the
/// modules' `mattr` defaults, the host's config and its queue adapter.
struct Boot<'a> {
    file: String,
    modules: HashMap<(String, String), Val>,
    config: HashMap<Vec<String>, Val>,
    namespaces: &'a [Forwarder],
    queue_adapter: Val,
    dead: Vec<(String, u32, u32)>,
}

impl Boot<'_> {
    fn statements(&mut self, nodes: &[Node<'_>]) {
        for node in nodes {
            self.statement(node);
        }
    }

    fn branch(&mut self, node: Option<Node<'_>>) {
        if let Some(node) = node {
            let stmts = match node.as_statements_node() {
                Some(s) => s.body().iter().collect(),
                None => node.as_else_node().and_then(|e| e.statements()).map(|s| s.body().iter().collect()).unwrap_or_else(|| vec![node]),
            };
            self.statements(&stmts);
        }
    }

    fn kill(&mut self, node: Option<Node<'_>>) {
        if let Some(node) = node {
            let loc = node.location();
            self.dead.push((self.file.clone(), loc.start_offset() as u32, loc.end_offset() as u32));
        }
    }

    /// Both branches may run: what either writes is no longer known.
    fn either(&mut self, a: Option<Node<'_>>, b: Option<Node<'_>>) {
        let before = (self.modules.clone(), self.config.clone());
        self.branch(a);
        let after_a = (std::mem::replace(&mut self.modules, before.0.clone()), std::mem::replace(&mut self.config, before.1.clone()));
        self.branch(b);
        for (k, v) in after_a.0 {
            if self.modules.get(&k) != Some(&v) {
                self.modules.insert(k, Val::Unknown);
            }
        }
        for (k, v) in after_a.1 {
            if self.config.get(&k) != Some(&v) {
                self.config.insert(k, Val::Unknown);
            }
        }
    }

    fn statement(&mut self, node: &Node<'_>) {
        if let Some(n) = node.as_if_node() {
            let then = n.statements().map(|s| s.as_node());
            let other = n.subsequent();
            match self.value(&n.predicate()).truthy() {
                Some(true) => {
                    self.kill(other);
                    self.branch(then);
                }
                Some(false) => {
                    self.kill(then);
                    self.branch(other);
                }
                None => self.either(then, other),
            }
            return;
        }
        if let Some(n) = node.as_unless_node() {
            let then = n.statements().map(|s| s.as_node());
            let other = n.else_clause().map(|e| e.as_node());
            match self.value(&n.predicate()).truthy() {
                Some(false) => {
                    self.kill(other);
                    self.branch(then);
                }
                Some(true) => {
                    self.kill(then);
                    self.branch(other);
                }
                None => self.either(then, other),
            }
            return;
        }
        if let Some(w) = node.as_call_or_write_node() {
            if let Some(path) = w.receiver().and_then(|r| config_chain(&r)) {
                let mut key = path;
                key.push(constant_id_str(&w.read_name()).to_string());
                let current = self.config_value(&key);
                let next = match current.truthy() {
                    Some(true) => current,
                    Some(false) => self.value(&w.value()),
                    None => Val::Unknown,
                };
                self.config.insert(key, next);
            }
            return;
        }
        let Some(call) = node.as_call_node() else { return };
        let name = constant_id_str(&call.name()).to_string();
        let args: Vec<Node<'_>> = call.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default();
        if let Some((path, module)) = forwarder(node) {
            if self.namespaces.iter().any(|f| f.path == path && f.module == module) {
                let copied: Vec<(String, Val)> = self
                    .config
                    .iter()
                    .filter(|(k, _)| k.len() == path.len() + 1 && k.starts_with(&path))
                    .map(|(k, v)| (k[path.len()].clone(), v.clone()))
                    .collect();
                for (attr, value) in copied {
                    self.modules.insert((module.clone(), attr), value);
                }
                return;
            }
        }
        let Some(receiver) = call.receiver() else { return };
        if let Some(setter) = name.strip_suffix('=').filter(|n| !n.is_empty() && args.len() == 1) {
            let value = self.value(&args[0]);
            if let Some(module) = constant_path_of(&receiver) {
                self.modules.insert((module.join("::"), setter.to_string()), value);
            } else if let Some(mut path) = config_chain(&receiver) {
                path.push(setter.to_string());
                self.config.insert(path, value);
            }
            return;
        }
        let Some(key) = self.module_reader(&receiver) else { return };
        let current = self.modules.get(&key).cloned().unwrap_or(Val::Unknown);
        let next = match (name.as_str(), current, args.as_slice()) {
            ("<<" | "add", Val::Set(mut members), [member]) => match self.value(member) {
                Val::Unknown => Val::Unknown,
                member => {
                    if !members.contains(&member) {
                        members.push(member);
                    }
                    Val::Set(members)
                }
            },
            (name, current, _) if !name.ends_with('!') && !matches!(name, "<<" | "add" | "push" | "concat" | "clear" | "delete" | "replace" | "merge" | "update") => current,
            _ => Val::Unknown,
        };
        self.modules.insert(key, next);
    }

    /// `(module, attr)` for `Module.attr`.
    fn module_reader(&self, node: &Node<'_>) -> Option<(String, String)> {
        let call = node.as_call_node()?;
        if call.arguments().is_some() || call.block().is_some() {
            return None;
        }
        let module = constant_path_of(&call.receiver()?)?.join("::");
        Some((module, constant_id_str(&call.name()).to_string()))
    }

    fn config_value(&self, path: &[String]) -> Val {
        if path == ["active_job", "queue_adapter"] {
            return self.queue_adapter.clone();
        }
        if let Some(v) = self.config.get(path) {
            return v.clone();
        }
        // An unset key of a namespace the engine made an OrderedOptions reads as nil.
        if self.namespaces.iter().any(|f| path.len() == f.path.len() + 1 && path.starts_with(&f.path)) {
            return Val::Nil;
        }
        Val::Unknown
    }

    fn value(&self, node: &Node<'_>) -> Val {
        if let Some(p) = node.as_parentheses_node() {
            let stmts: Vec<Node<'_>> = p.body().and_then(|b| b.as_statements_node()).map(|s| s.body().iter().collect()).unwrap_or_default();
            return match stmts.as_slice() {
                [only] => self.value(only),
                _ => Val::Unknown,
            };
        }
        if let Some(or) = node.as_or_node() {
            let left = self.value(&or.left());
            return match left.truthy() {
                Some(true) => left,
                Some(false) => self.value(&or.right()),
                None => Val::Unknown,
            };
        }
        if let Some(and) = node.as_and_node() {
            let left = self.value(&and.left());
            return match left.truthy() {
                Some(false) => left,
                Some(true) => self.value(&and.right()),
                None => Val::Unknown,
            };
        }
        let literal = literal_val(node);
        if literal != Val::Unknown {
            return literal;
        }
        let Some(call) = node.as_call_node() else { return Val::Unknown };
        if let Some(path) = config_chain(node).filter(|p| !p.is_empty()) {
            return self.config_value(&path);
        }
        if let Some(key) = self.module_reader(node) {
            return self.modules.get(&key).cloned().unwrap_or(Val::Unknown);
        }
        let name = constant_id_str(&call.name());
        let Some(receiver) = call.receiver() else { return Val::Unknown };
        let recv = self.value(&receiver);
        let args: Vec<Node<'_>> = call.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default();
        match (name, &recv, args.as_slice()) {
            ("!", _, []) => recv.truthy().map(|b| Val::Bool(!b)).unwrap_or(Val::Unknown),
            ("nil?", _, []) if recv != Val::Unknown => Val::Bool(recv == Val::Nil),
            ("empty?", Val::Set(members), []) => Val::Bool(members.is_empty()),
            ("empty?", Val::Str(s), []) => Val::Bool(s.is_empty()),
            ("include?", Val::Set(members), [member]) => match self.value(member) {
                Val::Unknown => Val::Unknown,
                member => Val::Bool(members.contains(&member)),
            },
            _ => Val::Unknown,
        }
    }
}

/// `config.active_job.queue_adapter` as `before_initialize` reads it: set
/// in `config/application.rb`, then the production environment file,
/// which both load before it runs; `Nil` when neither sets it.
fn host_queue_adapter<V: Vfs + ?Sized>(vfs: &V, dir: &Path) -> Val {
    let mut value = Val::Nil;
    for file in [APPLICATION, "config/environments/production.rb"] {
        let Ok(source) = vfs.read(&dir.join(file)) else { continue };
        let parsed = super::prism::parse_silent(&source);
        struct Writes(Vec<Val>);
        impl<'pr> ruby_prism::Visit<'pr> for Writes {
            fn visit_call_node(&mut self, call: &ruby_prism::CallNode<'pr>) {
                let is_write = constant_id_str(&call.name()) == "queue_adapter="
                    && call.receiver().and_then(|r| config_chain(&r).or_else(|| app_config_chain(&r))).is_some_and(|p| p == ["active_job"]);
                if is_write {
                    let args: Vec<Node<'_>> = call.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default();
                    self.0.push(match args.as_slice() {
                        [value] => literal_val(value),
                        _ => Val::Unknown,
                    });
                }
                ruby_prism::visit_call_node(self, call);
            }
        }
        let mut writes = Writes(Vec::new());
        ruby_prism::Visit::visit(&mut writes, &parsed.node());
        if let Some(last) = writes.0.pop() {
            // Not the last write alone: one under a condition may be the one that ran.
            value = if writes.0.iter().all(|w| *w == last) { last } else { Val::Unknown };
        }
    }
    value
}

/// `["active_job"]` for `Rails.application.config.active_job`.
fn app_config_chain(node: &Node<'_>) -> Option<Vec<String>> {
    let call = node.as_call_node()?;
    let receiver = call.receiver()?;
    let inner = receiver.as_call_node()?;
    if constant_id_str(&inner.name()) != "config" {
        return None;
    }
    let app = inner.receiver()?.as_call_node()?;
    let rails = app.receiver().and_then(|r| constant_path_of(&r))?;
    (constant_id_str(&app.name()) == "application" && rails == ["Rails"]).then(|| vec![constant_id_str(&call.name()).to_string()])
}

/// The spans of the branches each engine's `before_initialize` does not take.
fn dead_branches<V: Vfs + ?Sized>(vfs: &V, dir: &Path, libs: &[PathBuf], forwarders: &[Forwarder]) -> Vec<(String, u32, u32)> {
    let queue_adapter = host_queue_adapter(vfs, dir);
    let mut config: HashMap<Vec<String>, Val> = HashMap::default();
    if let Ok(source) = vfs.read(&dir.join(APPLICATION)) {
        let parsed = super::prism::parse_silent(&source);
        for (_, class) in find_all_classes_with_scope(&parsed.node()) {
            let is_application = class.superclass().and_then(|s| constant_path_of(&s)).is_some_and(|p| p == ["Rails", "Application"]);
            let Some(body) = class.body().filter(|_| is_application) else { continue };
            for stmt in flatten_statements(body) {
                let Some(call) = stmt.as_call_node() else { continue };
                let name = constant_id_str(&call.name());
                let Some(attr) = name.strip_suffix('=').filter(|a| !a.is_empty()) else { continue };
                let Some(mut path) = call.receiver().and_then(|r| config_chain(&r)) else { continue };
                if !forwarders.iter().any(|f| f.path == path) {
                    continue;
                }
                let args: Vec<Node<'_>> = call.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default();
                path.push(attr.to_string());
                config.insert(path, args.first().map(literal_val).unwrap_or(Val::Unknown));
            }
        }
    }
    let mut dead = Vec::new();
    for lib in libs {
        if !vfs.is_dir(lib) {
            continue;
        }
        for file in super::app::read_rb_files(vfs, lib).unwrap_or_default() {
            let Ok(source) = vfs.read(&file) else { continue };
            if !contains(&source, b"Engine") || !contains(&source, b"before_initialize") {
                continue;
            }
            let lib_sources: Vec<Vec<u8>> = super::app::read_rb_files(vfs, lib).unwrap_or_default().iter().filter_map(|f| vfs.read(f).ok()).collect();
            let parsed = super::prism::parse_silent(&source);
            for (_, class) in find_all_classes_with_scope(&parsed.node()) {
                let is_engine = class.superclass().and_then(|s| constant_path_of(&s)).is_some_and(|p| p == ["Rails", "Engine"]);
                let Some(body) = class.body().filter(|_| is_engine) else { continue };
                let mut modules = HashMap::default();
                for fw in forwarders.iter().filter(|f| f.lib == *lib) {
                    for (attr, value) in defaults(&lib_sources, &fw.module) {
                        modules.insert((fw.module.clone(), attr), value);
                    }
                }
                let mut boot = Boot {
                    file: file.display().to_string(),
                    modules,
                    config: config.clone(),
                    namespaces: forwarders,
                    queue_adapter: queue_adapter.clone(),
                    dead: Vec::new(),
                };
                for stmt in flatten_statements(body) {
                    if let Some(hook) = config_hook(&stmt, "before_initialize") {
                        boot.statements(&hook);
                    }
                }
                dead.extend(boot.dead);
            }
        }
    }
    dead
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_is_a_whole_word() {
        assert_eq!(mentions(b"Jobs.base = 1\nJobs.base_x = 2\nX::Jobs.base\n", "Jobs.base"), 1);
    }
}
