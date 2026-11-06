//! The Rack middleware a source gem's Railtie puts on the app's stack.
//!
//! A gem that is a Rack middleware inserts itself from its Railtie:
//! rack-attack's is `initializer "rack-attack.middleware" do |app|
//! app.middleware.use(Rack::Attack) end`. Rails runs it at boot, so in
//! a Rails app the middleware sees every request. The emitted trees run
//! no Railtie, so this reads the insertion from the gem's own source and
//! records the class, in the order Rails would `use` it, for both
//! ruby-family boots to stack (`App::gem_middlewares`).
//!
//! `use` appends: the middleware sits after Rails' own (the static file
//! server, `ActionDispatch::RequestId`, the session), just before the
//! routes. Only `use` with the class alone is read; a middleware given
//! arguments or placed with `insert_before` / `insert_after` is
//! reported as unsupported instead of left out.

use crate::det::HashSet;
use std::path::{Path, PathBuf};

use ruby_prism::Node;

use crate::diagnostic::Diagnostic;
use crate::span::Span;
use crate::vfs::Vfs;

use super::prism_api::PrismLocation;
use super::util::{
    class_name_path, constant_id_str, constant_path_is_rooted, constant_path_of, find_all_classes_with_nesting,
    find_all_module_declarations_with_scope, flatten_statements, module_name_path,
};

/// A `use(K)` as written, before `K` is resolved where the Railtie's body names it.
struct Use {
    path: Vec<String>,
    rooted: bool,
    /// `Module.nesting` inside the Railtie's body, innermost first.
    lexical: Vec<String>,
    span: Span,
}

pub(super) fn read<V: Vfs + ?Sized>(vfs: &V, gem_libs: &[PathBuf]) -> (Vec<String>, Vec<Diagnostic>) {
    let mut out = Vec::new();
    let mut diags = Vec::new();
    for lib in gem_libs {
        let mut files = Vec::new();
        rb_files(vfs, lib, &mut files);
        files.sort();
        let mut uses = Vec::new();
        for file in &files {
            let Ok(bytes) = vfs.read(file) else { continue };
            // Not every file: only one that names a railtie can insert anything.
            if !bytes.windows(7).any(|w| w == b"Railtie" || w == b"::Engin") {
                continue;
            }
            let parsed = super::prism::parse_silent(&bytes);
            let root = parsed.node();
            let name = file.display().to_string();
            for (scope, nesting, class) in find_all_classes_with_nesting(&root) {
                let railtie = class
                    .superclass()
                    .and_then(|s| constant_path_of(&s))
                    .is_some_and(|p| matches!(p.join("::").as_str(), "Rails::Railtie" | "Rails::Engine"));
                if !railtie {
                    continue;
                }
                let Some(body) = class.body() else { continue };
                let own = scope.iter().cloned().chain(class_name_path(&class).unwrap_or_default()).collect::<Vec<_>>();
                let lexical: Vec<String> = std::iter::once(own.join("::")).chain(nesting).collect();
                // Not `file_id` alone: a file library ingest has not registered yet answers no file.
                let file_id = super::sources::register(&name, &String::from_utf8_lossy(&bytes));
                for stmt in flatten_statements(body) {
                    read_initializer(&stmt, file_id, &lexical, &mut uses, &mut diags);
                }
            }
        }
        if uses.is_empty() {
            continue;
        }
        let defined = defined_constants(vfs, &files);
        for u in uses {
            match resolve(&u, &defined) {
                Some(class) => out.push(class),
                None => diags.push(Diagnostic::unsupported(
                    u.span,
                    None,
                    "railtie middleware",
                    format!("`{}` is no class this gem defines", u.path.join("::")),
                )),
            }
        }
    }
    (out, diags)
}

/// Ruby's lexical lookup: each enclosing body innermost first, then the top level.
fn resolve(u: &Use, defined: &HashSet<String>) -> Option<String> {
    let written = u.path.join("::");
    let crefs = if u.rooted { &[][..] } else { &u.lexical[..] };
    crefs
        .iter()
        .map(|cref| format!("{cref}::{written}"))
        .chain(std::iter::once(written.clone()))
        .find(|name| defined.contains(name))
}

fn defined_constants<V: Vfs + ?Sized>(vfs: &V, files: &[PathBuf]) -> HashSet<String> {
    let mut defined = HashSet::default();
    for file in files {
        let Ok(bytes) = vfs.read(file) else { continue };
        let parsed = super::prism::parse_silent(&bytes);
        let root = parsed.node();
        for (scope, _, class) in find_all_classes_with_nesting(&root) {
            if let Some(path) = class_name_path(&class) {
                defined.insert(scope.iter().cloned().chain(path).collect::<Vec<_>>().join("::"));
            }
        }
        for (scope, module) in find_all_module_declarations_with_scope(&root) {
            if let Some(path) = module_name_path(&module) {
                defined.insert(scope.iter().cloned().chain(path).collect::<Vec<_>>().join("::"));
            }
        }
    }
    defined
}

fn rb_files<V: Vfs + ?Sized>(vfs: &V, dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = vfs.read_dir(dir) else { return };
    for entry in entries {
        if vfs.is_dir(&entry) {
            rb_files(vfs, &entry, out);
        } else if entry.extension().is_some_and(|e| e == "rb") {
            out.push(entry);
        }
    }
}

/// `initializer "…" do |app| app.middleware.use(K) end`.
fn read_initializer(
    stmt: &Node<'_>,
    file: crate::span::FileId,
    lexical: &[String],
    uses: &mut Vec<Use>,
    diags: &mut Vec<Diagnostic>,
) {
    let Some(call) = stmt.as_call_node() else { return };
    if constant_id_str(&call.name()) != "initializer" || call.receiver().is_some() {
        return;
    }
    let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
    let app = block
        .parameters()
        .and_then(|p| p.as_block_parameters_node())
        .and_then(|p| p.parameters())
        .and_then(|p| p.requireds().iter().next())
        .and_then(|p| p.as_required_parameter_node().map(|r| constant_id_str(&r.name()).to_string()));
    let Some(app) = app else { return };
    let Some(body) = block.body() else { return };
    for inner in flatten_statements(body) {
        let Some(use_call) = inner.as_call_node() else { continue };
        let verb = constant_id_str(&use_call.name());
        if !matches!(verb, "use" | "insert_before" | "insert_after" | "insert" | "unshift" | "swap") {
            continue;
        }
        let Some(stack) = use_call.receiver() else { continue };
        if !is_middleware_of(&stack, &app) {
            continue;
        }
        let loc = inner.location();
        let span = Span { file, start: loc.start_offset() as u32, end: loc.end_offset() as u32 };
        let args: Vec<Node<'_>> = use_call.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default();
        let class = match (verb, args.as_slice()) {
            ("use", [only]) if use_call.block().is_none() => constant_path_of(only).map(|path| {
                let rooted = only.as_constant_path_node().is_some_and(|p| constant_path_is_rooted(&p));
                (path, rooted)
            }),
            _ => None,
        };
        match class {
            Some((path, rooted)) => uses.push(Use { path, rooted, lexical: lexical.to_vec(), span }),
            None => diags.push(Diagnostic::unsupported(
                span,
                None,
                "railtie middleware",
                format!("`{verb}` with these arguments: only `app.middleware.use(Klass)` is put on the emitted stack"),
            )),
        }
    }
}

/// `app.middleware` or `app.config.middleware`.
fn is_middleware_of(node: &Node<'_>, app: &str) -> bool {
    let Some(call) = node.as_call_node() else { return false };
    if constant_id_str(&call.name()) != "middleware" || call.arguments().is_some() {
        return false;
    }
    let Some(recv) = call.receiver() else { return false };
    let is_app = |n: &Node<'_>| n.as_local_variable_read_node().is_some_and(|r| constant_id_str(&r.name()) == app);
    if is_app(&recv) {
        return true;
    }
    recv.as_call_node().is_some_and(|c| constant_id_str(&c.name()) == "config" && c.receiver().is_some_and(|r| is_app(&r)))
}
