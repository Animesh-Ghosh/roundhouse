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
    let mut out = Vec::new();
    for source in sources {
        let parsed = super::prism::parse_silent(source);
        let mut stack = Vec::new();
        modules(&parsed.node(), &mut stack, module, &mut out);
    }
    out
}

fn modules(node: &Node<'_>, stack: &mut Vec<String>, wanted: &str, out: &mut Vec<(String, String)>) {
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
                out.extend(string_default(&stmt));
            }
            modules(&stmt, stack, wanted, out);
        }
    }
    stack.truncate(depth);
}

fn string_default(node: &Node<'_>) -> Vec<(String, String)> {
    let Some(call) = node.as_call_node() else { return Vec::new() };
    if call.receiver().is_some() || !matches!(constant_id_str(&call.name()), "mattr_accessor" | "cattr_accessor" | "mattr_reader" | "cattr_reader") {
        return Vec::new();
    }
    let args: Vec<Node<'_>> = call.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default();
    let mut names = Vec::new();
    let mut default = None;
    for arg in &args {
        if let Some(sym) = arg.as_symbol_node() {
            names.push(String::from_utf8_lossy(sym.unescaped()).into_owned());
        } else if let Some(hash) = arg.as_keyword_hash_node() {
            for element in hash.elements().iter() {
                let Some(pair) = element.as_assoc_node() else { continue };
                let is_default = pair.key().as_symbol_node().is_some_and(|k| k.unescaped() == b"default");
                if is_default {
                    default = pair.value().as_string_node().map(|s| String::from_utf8_lossy(s.unescaped()).into_owned());
                }
            }
        }
    }
    let Some(default) = default else { return Vec::new() };
    names.into_iter().map(|n| (n, default.clone())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_is_a_whole_word() {
        assert_eq!(mentions(b"Jobs.base = 1\nJobs.base_x = 2\nX::Jobs.base\n", "Jobs.base"), 1);
    }
}
