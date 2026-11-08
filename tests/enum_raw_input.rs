//! Rails answers `<enum>_before_type_cast` with what was assigned until
//! the record saves or loads, and the stored value after (activerecord
//! 8.1.4). The ruby family keeps the assigned value; strict targets refuse.
#[path = "support/emit_and_run.rs"]
mod emit_and_run;

fn raw_state_app() -> emit_and_run::Overlay {
    emit_and_run::real_blog()
        .edit(
            "db/schema.rb",
            "create_table \"articles\", force: :cascade do |t|",
            "create_table \"articles\", force: :cascade do |t|\n    t.integer \"state\"\n    t.string \"tone\"\n    t.integer \"level\", default: 0, null: false",
        )
        .edit(
            "app/models/article.rb",
            "class Article < ApplicationRecord\n",
            "class Article < ApplicationRecord\n  enum :state, { draft: 0, published: 1 }\n  enum :tone, { quiet: \"q\", loud: \"l\" }\n  enum :level, { low: 0, high: 1 }\n\n  def raw_state\n    state_before_type_cast\n  end\n\n  def raw_tone\n    tone_before_type_cast\n  end\n\n  def raw_level\n    level_before_type_cast\n  end\n",
        )
}

const ANSWERS_TEST: &str = r#"require "test_helper"

class ArticleRawStateTest < ActiveSupport::TestCase
  ATTRS = { title: "Raw", body: "A body long enough to validate." }

  test "an unsaved record answers what was assigned" do
    assert_nil Article.new.raw_state
    assert_equal :published, Article.new(state: :published).raw_state
    assert_equal "published", Article.new(state: "published").raw_state
    assert_equal 1, Article.new(state: 1).raw_state
    assert_nil Article.new(state: nil).raw_state
    assert_equal :loud, Article.new(tone: :loud).raw_tone
    assert_equal "l", Article.new(tone: "l").raw_tone
    article = Article.new
    article.state = :published
    assert_equal :published, article.raw_state
    article.state = :draft
    assert_equal :draft, article.raw_state
  end

  test "a schema default reads as its text" do
    assert_equal "0", Article.new.raw_level
  end

  test "a saved or loaded record answers the stored value" do
    article = Article.create!(ATTRS.merge(state: :published, tone: :loud))
    assert_equal 1, article.raw_state
    assert_equal "l", article.raw_tone
    assert_equal 1, article.reload.raw_state
    found = Article.find(article.id)
    assert_equal 1, found.raw_state
    assert_equal "l", found.raw_tone
    assert_equal 0, found.raw_level
    found.state = :draft
    assert_equal :draft, found.raw_state
    found.save!
    assert_equal 0, found.raw_state
    found.state = :draft
    found.save!
    assert_equal 0, found.raw_state
  end

  test "a bang writer saves" do
    article = Article.new(ATTRS)
    article.published!
    assert_equal 1, article.raw_state
  end
end
"#;

#[test]
fn enum_before_type_cast_answers_as_rails_does() {
    raw_state_app()
        .write("test/models/article_raw_state_test.rb", ANSWERS_TEST)
        .run_test("test/models/article_raw_state_test.rb")
        .assert_passes();
}

/// A label written on a `Model.new` in a controller stays as assigned when
/// only that controller reads the raw value (`:published` in Rails 8.1.4).
#[test]
fn a_controller_write_keeps_the_assigned_label() {
    emit_and_run::real_blog()
        .edit("db/schema.rb", "create_table \"articles\", force: :cascade do |t|",
            "create_table \"articles\", force: :cascade do |t|\n    t.integer \"state\"")
        .edit("app/models/article.rb", "class Article < ApplicationRecord\n",
            "class Article < ApplicationRecord\n  enum :state, { draft: 0, published: 1 }\n")
        .edit("config/routes.rb", "  root \"articles#index\"\n",
            "  root \"articles#index\"\n  get \"/raw_probe\", to: \"raw_probes#show\"\n")
        .write("app/controllers/raw_probes_controller.rb", r#"class RawProbesController < ApplicationController
  def show
    article = Article.new(title: "t", body: "a body long enough", state: :published)
    render plain: article.state_before_type_cast.inspect
  end
end
"#)
        .write("test/controllers/raw_probes_controller_test.rb", r#"require "test_helper"

class RawProbesControllerTest < ActionDispatch::IntegrationTest
  test "the raw reader answers the assigned label" do
    get "/raw_probe"
    assert_equal ":published", response.body
  end
end
"#)
        .run_test("test/controllers/raw_probes_controller_test.rb")
        .assert_passes();
}

/// An enum's `default:` reads as its label, as Rails 8.1.4 does.
#[test]
fn an_enum_default_reads_as_its_label() {
    emit_and_run::real_blog()
        .edit("db/schema.rb", "create_table \"articles\", force: :cascade do |t|",
            "create_table \"articles\", force: :cascade do |t|\n    t.integer \"state\"")
        .edit("app/models/article.rb", "class Article < ApplicationRecord\n",
            "class Article < ApplicationRecord\n  enum :state, { draft: 0, published: 1 }, default: :published\n  def raw_state\n    state_before_type_cast\n  end\n")
        .run_ruby("raise Article.new.raw_state.inspect unless Article.new.raw_state == :published")
        .assert_passes();
}

#[test]
fn strict_targets_refuse_enum_before_type_cast() {
    let (_, errors) = raw_state_app().emit(roundhouse::project::BuildTarget::Go);
    assert!(
        errors.iter().any(|e| e.contains("enum_before_type_cast not supported (go)")),
        "the value as assigned must not be claimed on Go: {errors:?}",
    );
}

#[test]
fn source_defined_before_type_cast_methods_keep_their_behavior() {
    emit_and_run::real_blog()
        .edit("db/schema.rb", "create_table \"articles\", force: :cascade do |t|",
            "create_table \"articles\", force: :cascade do |t|\n    t.integer \"state\"")
        .write("app/models/concerns/raw_state.rb", "module RawState\n  def state_before_type_cast\n    \"custom\"\n  end\nend\n")
        .edit("app/models/article.rb", "class Article < ApplicationRecord\n",
            "class Article < ApplicationRecord\n  enum :state, { draft: 0, published: 1 }\n  include RawState\n  def raw_state\n    state_before_type_cast\n  end\n")
        .run_ruby("raise 'custom raw state' unless Article.new.raw_state == 'custom'")
        .assert_passes();
}
