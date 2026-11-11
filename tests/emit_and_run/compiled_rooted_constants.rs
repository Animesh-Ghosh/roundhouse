//! A constant written from the top level (`::Article`) names the same
//! class as a bare `Article` on a compiled target: they have no lexical
//! constant lookup for the root to choose between. The Ruby family and
//! Crystal print `::`; TypeScript, Python and Elixir have no such
//! syntax, so their emit of a rooted constant must be the bare one's.

use roundhouse::project::BuildTarget;

use super::emit_and_run;

const TALLY: &str = "class Tally\n  def initialize(n)\n    @n = n\n  end\n\n  def n\n    @n\n  end\nend\n";

const INDEX: &str = "    @articles = Article.includes(:comments).order(created_at: :desc)\n";

fn app(root: &str) -> emit_and_run::Overlay {
    emit_and_run::real_blog()
        .write("app/models/tally.rb", TALLY)
        .edit(
            "app/models/article.rb",
            "  validates :title, presence: true\n",
            "  validates :title, presence: true\n\n  def self.total\n    Article.count\n  end\n",
        )
        .edit(
            "app/controllers/articles_controller.rb",
            INDEX,
            &format!("{INDEX}    @total = {root}Article.total\n    @tally = {root}Tally.new(2).n\n"),
        )
}

/// Every emitted file whose text differs between the rooted and the bare
/// app. A TypeScript source map is left out: it carries the Ruby source as
/// written, `::` and its columns included.
fn differing_files(target: BuildTarget) -> Vec<String> {
    let (rooted, _) = app("::").emit(target);
    let (bare, _) = app("").emit(target);
    let mut out = Vec::new();
    for entry in walk(&bare).into_iter().filter(|p| p.extension().is_none_or(|e| e != "map")) {
        let rel = entry.strip_prefix(&*bare).unwrap();
        let theirs = std::fs::read(rooted.join(rel)).unwrap_or_default();
        if std::fs::read(&entry).unwrap() != theirs {
            let text = String::from_utf8_lossy(&theirs);
            let lines: Vec<&str> = text.lines().filter(|l| l.contains("Article") || l.contains("Tally")).collect();
            out.push(format!("{}:\n{}", rel.display(), lines.join("\n")));
        }
    }
    out
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.file_name().is_some_and(|n| n == "node_modules" || n == "_build" || n == "deps") {
            continue;
        }
        if path.is_dir() {
            files.extend(walk(&path));
        } else {
            files.push(path);
        }
    }
    files
}

#[test]
fn typescript_emits_a_rooted_constant_as_the_bare_one() {
    let differing = differing_files(BuildTarget::Typescript);
    assert!(differing.is_empty(), "{}", differing.join("\n\n"));
}

#[test]
fn python_emits_a_rooted_constant_as_the_bare_one() {
    let differing = differing_files(BuildTarget::Python);
    assert!(differing.is_empty(), "{}", differing.join("\n\n"));
}

#[test]
fn elixir_emits_a_rooted_constant_as_the_bare_one() {
    let differing = differing_files(BuildTarget::Elixir);
    assert!(differing.is_empty(), "{}", differing.join("\n\n"));
}

/// The Ruby family keeps the root: inside a namespace it picks the
/// top-level class, which a bare name would not.
#[test]
fn ruby_keeps_the_root() {
    let (rooted, _) = app("::").emit(BuildTarget::Ruby);
    let controller = std::fs::read_to_string(rooted.join("app/controllers/articles_controller.rb")).unwrap();
    assert!(controller.contains("::Article.total") && controller.contains("::Tally.new(2)"), "{controller}");
}
