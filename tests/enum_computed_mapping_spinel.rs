//! The Spinel side of `tests/enum_computed_mapping.rs`. Needs a Spinel
//! compiler, so `#[ignore]`d; CI's `spinel-framework` job runs it
//! (`scripts/ci-plan.py`'s `SPINEL_TESTS`).

#[path = "support/emit_and_run.rs"]
#[allow(dead_code)]
mod emit_and_run;

const APPLICATION_RECORD: &str = "class ApplicationRecord < ActiveRecord::Base\n  primary_abstract_class\nend\n";

fn app(mapping: &str) -> emit_and_run::Overlay {
    emit_and_run::empty_app()
        .write(
            "db/schema.rb",
            "ActiveRecord::Schema.define do\n  create_table \"widgets\", force: :cascade do |t|\n    t.integer \"state\", default: 0, null: false\n  end\nend\n",
        )
        .write("config/routes.rb", "Rails.application.routes.draw do\nend\n")
        .write("app/models/application_record.rb", APPLICATION_RECORD)
        .write(
            "app/models/widget.rb",
            &format!("class Widget < ApplicationRecord\n  BASE = %i[draft live].freeze\n  enum :state, {mapping}\nend\n"),
        )
}

const FOLDS: &[&str] = &[
    "(BASE.dup << %i[archived]).flatten",
    "BASE + %i[archived]",
    "BASE.dup << :archived",
    "(BASE + %w[archived]).flatten.freeze",
];

const SCRIPT: &str = r#"
got = Widget.states
want = { "draft" => 0, "live" => 1, "archived" => 2 }
raise "states: #{got.inspect}" unless got == want
w = Widget.create!
raise "default: #{w.state}" unless w.state == "draft"
w.archived!
raise "bang: #{w.state}" unless Widget.find(w.id).archived?
puts "computed enum passed"
"#;

#[test]
#[ignore = "requires the Spinel toolchain"]
fn a_mapping_computed_from_frozen_lists_folds_on_spinel() {
    app(FOLDS[0]).run_spinel_with_db(SCRIPT).assert_passes();
}
