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
    /// The initializer names `before:` or `after:`, an order this reader does not keep.
    ordered: bool,
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
            // Not every Railtie the gem ships: Rails runs one only once something loads its file.
            let loaded = loading(vfs, lib, &files, file);
            if loaded == Loading::Never {
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
                if matches!(loaded, Loading::Autoload | Loading::Unknown) {
                    let loc = class.location();
                    let why = if loaded == Loading::Autoload {
                        "a Railtie only autoloaded: whether the app loads it, and so runs its initializers, is not known here"
                    } else {
                        "a Railtie in a gem with no entry file to follow: whether Bundler's require loads it is not known here"
                    };
                    diags.push(Diagnostic::unsupported(
                        Span { file: file_id, start: loc.start_offset() as u32, end: loc.end_offset() as u32 },
                        None,
                        "railtie middleware",
                        why.to_string(),
                    ));
                    continue;
                }
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
                Some(class) => out.push((class, u.ordered, u.span)),
                None => diags.push(Diagnostic::unsupported(
                    u.span,
                    None,
                    "railtie middleware",
                    format!("`{}` is no class this gem defines", u.path.join("::")),
                )),
            }
        }
    }
    // Not stacked in reading order when initializers order themselves: Rails sorts them by `before:` and `after:`.
    if out.len() > 1 {
        for (_, _, span) in out.iter().filter(|(_, ordered, _)| *ordered) {
            diags.push(Diagnostic::unsupported(
                *span,
                None,
                "railtie middleware",
                "an initializer ordered with `before:` or `after:` beside another gem's middleware: the emitted stack keeps reading order".to_string(),
            ));
        }
    }
    (out.into_iter().map(|(class, _, _)| class).collect(), diags)
}

#[derive(PartialEq)]
enum Loading {
    Required,
    Autoload,
    Never,
    /// No entry file to start from, so whether Bundler's require reaches it is not known.
    Unknown,
}

/// Whether Bundler's `require` of the gem reaches `file`: from the entry file (`lib/<name>.rb`, or the
/// dashed name's nested path), through each `require` and `require_relative` it runs, transitively.
fn loading<V: Vfs + ?Sized>(vfs: &V, lib: &Path, files: &[PathBuf], file: &Path) -> Loading {
    let dir = lib.parent().and_then(|d| d.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    // `rack-attack-6.7.0`: the gem's name is what precedes its version.
    let name = match dir.rfind(|c: char| c == '-') {
        Some(i) if dir[i + 1..].starts_with(|c: char| c.is_ascii_digit()) => dir[..i].to_string(),
        _ => dir.clone(),
    };
    let entries: Vec<PathBuf> =
        [name.clone(), name.replace('-', "/")].iter().map(|n| lib.join(format!("{n}.rb"))).filter(|p| files.contains(p)).collect();
    if entries.is_empty() {
        return Loading::Unknown;
    }
    let mut reached: Vec<PathBuf> = entries.clone();
    let mut autoloaded: Vec<PathBuf> = Vec::new();
    let mut queue = entries;
    while let Some(at) = queue.pop() {
        let Ok(bytes) = vfs.read(&at) else { continue };
        for line in String::from_utf8_lossy(&bytes).lines() {
            let line = line.trim_start();
            let Some(quoted) = line.split(['"', '\'']).nth(1) else { continue };
            let target = if line.starts_with("require_relative") {
                normalize(&at.parent().unwrap_or(lib).join(quoted))
            } else if line.starts_with("require") || line.starts_with("autoload") {
                lib.join(quoted)
            } else {
                continue;
            };
            let target = if target.extension().is_some_and(|e| e == "rb") { target } else { target.with_extension("rb") };
            if !files.contains(&target) {
                continue;
            }
            if line.starts_with("autoload") {
                autoloaded.push(target);
            } else if !reached.contains(&target) {
                reached.push(target.clone());
                queue.push(target);
            }
        }
    }
    if reached.iter().any(|p| p == file) {
        Loading::Required
    } else if autoloaded.iter().any(|p| p == file) {
        Loading::Autoload
    } else {
        Loading::Never
    }
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
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
    let ordered = call.arguments().is_some_and(|a| {
        a.arguments().iter().any(|arg| {
            arg.as_keyword_hash_node().is_some_and(|h| {
                h.elements().iter().any(|e| {
                    e.as_assoc_node()
                        .and_then(|a| a.key().as_symbol_node())
                        .is_some_and(|k| matches!(&*String::from_utf8_lossy(k.unescaped()), "before" | "after"))
                })
            })
        })
    });
    let Some(body) = block.body() else { return };
    for inner in flatten_statements(body) {
        let loc = inner.location();
        let span = Span { file, start: loc.start_offset() as u32, end: loc.end_offset() as u32 };
        let direct = inner.as_call_node().filter(|c| {
            matches!(constant_id_str(&c.name()), "use" | "insert_before" | "insert_after" | "insert" | "unshift" | "swap")
                && c.receiver().is_some_and(|stack| is_middleware_of(&stack, &app))
        });
        let Some(use_call) = direct else {
            // Not passed over: a middleware inserted under a condition, a loop or a hook is one Rails may run.
            if String::from_utf8_lossy(loc.as_slice()).contains(".middleware") {
                diags.push(Diagnostic::unsupported(
                    span,
                    None,
                    "railtie middleware",
                    "a middleware inserted under a condition or inside another statement: only a plain `app.middleware.use(Klass)` is put on the emitted stack".to_string(),
                ));
            }
            continue;
        };
        let verb = constant_id_str(&use_call.name());
        let args: Vec<Node<'_>> = use_call.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default();
        let class = match (verb, args.as_slice()) {
            ("use", [only]) if use_call.block().is_none() => constant_path_of(only).map(|path| {
                let rooted = only.as_constant_path_node().is_some_and(|p| constant_path_is_rooted(&p));
                (path, rooted)
            }),
            _ => None,
        };
        match class {
            Some((path, rooted)) => uses.push(Use { path, rooted, lexical: lexical.to_vec(), span, ordered }),
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::vfs::MapVfs;

    fn read(files: &[(&str, &str)]) -> (Vec<String>, Vec<String>) {
        let map = files.iter().map(|(path, text)| (PathBuf::from(path), text.as_bytes().to_vec())).collect();
        let (classes, diags) = super::read(&MapVfs::new(map), &[PathBuf::from("/gem/lib")]);
        (classes, diags.iter().map(|d| d.message.clone()).collect())
    }

    const ENTRY: &str = "module Gem\n  class Middleware\n  end\nend\nrequire \"gem/railtie\"\n";

    fn railtie(body: &str) -> String {
        format!("module Gem\n  class Railtie < Rails::Railtie\n{body}\n  end\nend\n")
    }

    #[test]
    fn a_required_railtie_stacks_its_middleware_by_its_lexical_name() {
        let rt = railtie("    initializer \"gem.middleware\" do |app|\n      app.middleware.use(Middleware)\n    end");
        let (classes, diags) = read(&[("/gem/lib/gem.rb", ENTRY), ("/gem/lib/gem/railtie.rb", &rt)]);
        assert_eq!(classes, ["Gem::Middleware"]);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn a_railtie_nothing_loads_inserts_nothing() {
        let rt = railtie("    initializer \"gem.middleware\" do |app|\n      app.middleware.use(Middleware)\n    end");
        let entry = ENTRY.replace("require \"gem/railtie\"\n", "");
        // Required only by a file the entry never loads.
        let unused = "require_relative \"railtie\"\n";
        let (classes, diags) =
            read(&[("/gem/lib/gem.rb", &entry), ("/gem/lib/gem/railtie.rb", &rt), ("/gem/lib/gem/unused.rb", unused)]);
        assert!(classes.is_empty() && diags.is_empty(), "{classes:?} {diags:?}");
        // Reached through a file the entry requires.
        let entry = ENTRY.replace("require \"gem/railtie\"", "require \"gem/unused\"");
        let (classes, _) =
            read(&[("/gem/lib/gem.rb", &entry), ("/gem/lib/gem/railtie.rb", &rt), ("/gem/lib/gem/unused.rb", unused)]);
        assert_eq!(classes, ["Gem::Middleware"]);
    }

    #[test]
    fn an_autoloaded_railtie_or_a_conditional_insertion_is_reported() {
        let rt = railtie("    initializer \"gem.middleware\" do |app|\n      app.middleware.use(Middleware)\n    end");
        let entry = ENTRY.replace("require \"gem/railtie\"", "autoload :Railtie, \"gem/railtie\"");
        let (classes, diags) = read(&[("/gem/lib/gem.rb", &entry), ("/gem/lib/gem/railtie.rb", &rt)]);
        assert!(classes.is_empty() && diags.len() == 1, "{classes:?} {diags:?}");
        let rt = railtie("    initializer \"gem.middleware\" do |app|\n      app.middleware.use(Middleware) if ENV[\"ON\"]\n    end");
        let (classes, diags) = read(&[("/gem/lib/gem.rb", ENTRY), ("/gem/lib/gem/railtie.rb", &rt)]);
        assert!(classes.is_empty() && diags.len() == 1, "{classes:?} {diags:?}");
    }

    #[test]
    fn an_unresolved_middleware_name_is_reported() {
        let rt = railtie("    initializer \"gem.middleware\" do |app|\n      app.middleware.use(Elsewhere)\n    end");
        let (classes, diags) = read(&[("/gem/lib/gem.rb", ENTRY), ("/gem/lib/gem/railtie.rb", &rt)]);
        assert!(classes.is_empty() && diags.len() == 1, "{classes:?} {diags:?}");
    }
}
