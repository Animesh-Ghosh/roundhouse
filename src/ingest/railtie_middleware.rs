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

use std::path::{Path, PathBuf};

use ruby_prism::Node;

use crate::diagnostic::Diagnostic;
use crate::span::Span;
use crate::vfs::Vfs;

use super::prism_api::PrismLocation;
use super::util::{constant_id_str, constant_path_of, find_all_classes_with_scope, flatten_statements};

pub(super) fn read<V: Vfs + ?Sized>(vfs: &V, gem_libs: &[PathBuf]) -> (Vec<String>, Vec<Diagnostic>) {
    let mut out = Vec::new();
    let mut diags = Vec::new();
    for lib in gem_libs {
        let mut files = Vec::new();
        rb_files(vfs, lib, &mut files);
        files.sort();
        for file in files {
            let Ok(bytes) = vfs.read(&file) else { continue };
            // Not every file: only one that names a railtie can insert anything.
            if !bytes.windows(7).any(|w| w == b"Railtie" || w == b"::Engin") {
                continue;
            }
            let parsed = super::prism::parse_silent(&bytes);
            let root = parsed.node();
            let name = file.display().to_string();
            for (_, class) in find_all_classes_with_scope(&root) {
                let railtie = class
                    .superclass()
                    .and_then(|s| constant_path_of(&s))
                    .is_some_and(|p| matches!(p.join("::").as_str(), "Rails::Railtie" | "Rails::Engine"));
                if !railtie {
                    continue;
                }
                let Some(body) = class.body() else { continue };
                for stmt in flatten_statements(body) {
                    read_initializer(&stmt, &name, &mut out, &mut diags);
                }
            }
        }
    }
    (out, diags)
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
fn read_initializer(stmt: &Node<'_>, file: &str, out: &mut Vec<String>, diags: &mut Vec<Diagnostic>) {
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
        let args: Vec<Node<'_>> = use_call.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default();
        let class = match (verb, args.as_slice()) {
            ("use", [only]) if use_call.block().is_none() => constant_path_of(only).map(|p| p.join("::")),
            _ => None,
        };
        match class {
            Some(class) => out.push(class),
            None => {
                let loc = inner.location();
                let span = Span {
                    file: super::sources::file_id(file),
                    start: loc.start_offset() as u32,
                    end: loc.end_offset() as u32,
                };
                diags.push(Diagnostic::unsupported(
                    span,
                    None,
                    "railtie middleware",
                    format!("`{verb}` with these arguments: only `app.middleware.use(Klass)` is put on the emitted stack"),
                ));
            }
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
