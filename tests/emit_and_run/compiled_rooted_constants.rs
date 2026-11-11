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
    differing_between(app("::"), app(""), target)
}

/// [`differing_files`] for any two apps.
fn differing_between(rooted: emit_and_run::Overlay, bare: emit_and_run::Overlay, target: BuildTarget) -> Vec<String> {
    let (rooted, _) = rooted.emit(target);
    let (bare, _) = bare.emit(target);
    let mut out = Vec::new();
    for entry in walk(&bare).into_iter().filter(|p| p.extension().is_none_or(|e| e != "map")) {
        let rel = entry.strip_prefix(&*bare).unwrap();
        let theirs = std::fs::read(rooted.join(rel)).unwrap_or_default();
        if std::fs::read(&entry).unwrap() != theirs {
            let text = String::from_utf8_lossy(&theirs);
            let lines: Vec<&str> = text.lines().filter(|l| l.contains("Article") || l.contains("Tally") || l.contains("Limits")).collect();
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

/// `Api::Article` beside the top-level `Article`, and `Api::Report`
/// reading each: `::Article` is the top-level class, a bare `Article`
/// inside `module Api` is `Api::Article` (`::Tally` has no namespace
/// twin).
fn namespaced_app() -> emit_and_run::Overlay {
    app("")
        .write("app/models/api/article.rb", "module Api\n  class Article\n    def self.total\n      \"inner\"\n    end\n  end\nend\n")
        .write(
            "app/models/api/report.rb",
            "module Api\n  class Report\n    def self.outer\n      ::Article.total\n    end\n\n    def self.inner\n      Article.total\n    end\n\n    def self.tally\n      ::Tally.new(2).n\n    end\n  end\nend\n",
        )
}

/// TypeScript declares `Api::Report` inside `export namespace Api`, where
/// a bare `Article` is `Api.Article`: dropping the root would read the
/// wrong class, so the constant is refused. `::Tally`, which no member
/// of `Api` hides, is written bare.
#[test]
fn typescript_refuses_a_top_level_constant_a_namespace_member_hides() {
    let (emitted, errors) = namespaced_app().emit(BuildTarget::Typescript);
    let refused: Vec<&String> = errors.iter().filter(|e| e.contains("`::Article` in `Api::Report`")).collect();
    assert_eq!(refused.len(), 1, "{errors:#?}");
    assert!(refused[0].contains("namespace Api"), "{}", refused[0]);
    assert!(!errors.iter().any(|e| e.contains("::Tally")), "{errors:#?}");
    let api = std::fs::read_to_string(emitted.join("app/models/api.ts")).unwrap();
    assert!(api.contains("return new Tally(2).n;") && !api.contains("return ."), "{api}");
}

/// Elixir declares every module at the top level (`defmodule
/// Admin.ReportsController`), so a bare `Article` there is the
/// top-level module, never `Admin.Article`.
#[test]
fn elixir_reads_a_rooted_constant_in_a_namespace_as_the_top_level_module() {
    let (emitted, _) = app("")
        .write(
            "app/models/admin/article.rb",
            "module Admin\n  class Article < ApplicationRecord\n    self.table_name = \"articles\"\n  end\nend\n",
        )
        .write(
            "app/controllers/admin/reports_controller.rb",
            "module Admin\n  class ReportsController < ApplicationController\n    def index\n      @outer = ::Article.total\n      render plain: @outer\n    end\n  end\nend\n",
        )
        .edit("config/routes.rb", "Rails.application.routes.draw do\n", "Rails.application.routes.draw do\n  namespace :admin do\n    resources :reports, only: :index\n  end\n")
        .emit(BuildTarget::Elixir);
    let controller = std::fs::read_to_string(emitted.join("lib/admin_reportscontroller.ex")).unwrap();
    assert!(controller.starts_with("defmodule Admin.ReportsController do"), "{controller}");
    assert!(controller.contains("record = %{record | outer: Article.total}"), "{controller}");
}

/// A rooted constant outside a method body: a fixture's ERB value
/// (`<%= ::Limits::TITLE %>`) is emitted as the bare one's too.
fn fixture_app(root: &str) -> emit_and_run::Overlay {
    emit_and_run::real_blog()
        .write("app/models/limits.rb", "module Limits\n  TITLE = \"Limited\"\nend\n")
        .edit(
            "test/fixtures/articles.yml",
            "  title: Getting Started with Rails\n",
            &format!("  title: <%= {root}Limits::TITLE %>\n"),
        )
}

#[test]
fn typescript_emits_a_rooted_constant_in_a_fixture_as_the_bare_one() {
    let differing = differing_between(fixture_app("::"), fixture_app(""), BuildTarget::Typescript);
    assert!(differing.is_empty(), "{}", differing.join("\n\n"));
}

#[test]
fn python_emits_a_rooted_constant_in_a_fixture_as_the_bare_one() {
    let differing = differing_between(fixture_app("::"), fixture_app(""), BuildTarget::Python);
    assert!(differing.is_empty(), "{}", differing.join("\n\n"));
}

#[test]
fn elixir_emits_a_rooted_constant_in_a_fixture_as_the_bare_one() {
    let differing = differing_between(fixture_app("::"), fixture_app(""), BuildTarget::Elixir);
    assert!(differing.is_empty(), "{}", differing.join("\n\n"));
}

/// A test class that declares its own `Article` while reading the
/// model as `::Article`. In Ruby the root picks the top-level model.
fn test_class_app() -> emit_and_run::Overlay {
    app("").write(
        "test/models/tally_test.rb",
        "require \"test_helper\"\n\nclass TallyTest < ActiveSupport::TestCase\n  class Article\n    def self.total\n      \"inner\"\n    end\n  end\n\n  test \"reads the model\" do\n    assert_equal 2, ::Article.count\n  end\n\n  test \"counts\" do\n    assert_equal 2, ::Tally.new(2).n\n  end\nend\n",
    )
}

/// TypeScript hoists a test class's own classes to the test file's
/// scope, where a bare `Article` is the test's `Article`, not the
/// model: dropping the root would read the wrong class, so the constant
/// is refused, as it is inside a namespace. `::Tally`, which the test
/// declares no twin of, is written bare.
#[test]
fn typescript_refuses_a_top_level_constant_a_test_class_hides() {
    let (emitted, errors) = test_class_app().emit(BuildTarget::Typescript);
    let refused: Vec<&String> = errors.iter().filter(|e| e.contains("`::Article` in `TallyTest`")).collect();
    assert_eq!(refused.len(), 1, "{errors:#?}");
    assert!(!errors.iter().any(|e| e.contains("::Tally")), "{errors:#?}");
    let test = std::fs::read_to_string(emitted.join("test/tally.test.ts")).unwrap();
    assert!(test.contains("new Tally(2).n"), "{test}");
}
