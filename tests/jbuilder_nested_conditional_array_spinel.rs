//! The Spinel side of `tests/jbuilder_nested_conditional_array.rs`. Needs
//! a Spinel compiler, so `#[ignore]`d; CI's `spinel-framework` job runs
//! it (`scripts/ci-plan.py`'s `SPINEL_TESTS`).

#[path = "support/emit_and_run.rs"]
#[allow(dead_code)]
mod emit_and_run;

const APPLICATION_RECORD: &str = "class ApplicationRecord < ActiveRecord::Base\n  primary_abstract_class\nend\n";

fn app() -> emit_and_run::Overlay {
    emit_and_run::empty_app()
        .write(
            "db/schema.rb",
            "ActiveRecord::Schema.define do\n  create_table \"widgets\", force: :cascade do |t|\n    t.string \"name\"\n    t.string \"email\"\n  end\nend\n",
        )
        .write("config/routes.rb", "Rails.application.routes.draw do\n  resources :widgets, only: :show\nend\n")
        .write("app/models/application_record.rb", APPLICATION_RECORD)
        .write(
            "app/models/widget.rb",
            "class Widget < ApplicationRecord\n  def codes\n    name.to_s.split(\",\")\n  end\n\n  def codes_ready?\n    !email.nil?\n  end\nend\n",
        )
        .write("app/controllers/application_controller.rb", "class ApplicationController < ActionController::API\nend\n")
        .write(
            "app/controllers/widgets_controller.rb",
            "class WidgetsController < ApplicationController\n  def show\n    @widget = Widget.find(params[:id])\n  end\nend\n",
        )
        .write(
            "app/views/widgets/show.json.jbuilder",
            r#"json.codes do
  json.array!(@widget.codes) if @widget.codes_ready?
end
json.name @widget.name
json.others do
  json.array!(@widget.codes) { |c| json.v c } unless @widget.codes_ready?
end
json.tail 1
"#,
        )
}

const SCRIPT: &str = r##"
def render_for(widget)
  controller = WidgetsController.new
  controller.params = { "id" => widget.id.to_s, "format" => "json" }
  controller.process_action(:show)
  controller.body
end
ready = Widget.create!(name: "x,y", email: "e")
got = render_for(ready)
raise "ready: #{got}" unless got == %q({"codes":["x","y"],"name":"x,y","tail":1})
pending = Widget.create!(name: "x,y")
got = render_for(pending)
raise "pending: #{got}" unless got == %q({"name":"x,y","others":[{"v":"x"},{"v":"y"}],"tail":1})
empty = Widget.create!(name: "", email: "e")
got = render_for(empty)
raise "empty: #{got}" unless got == %q({"codes":[],"name":"","tail":1})
puts "conditional array passed"
"##;

#[test]
#[ignore = "requires the Spinel toolchain"]
fn a_conditional_array_is_the_keys_value_or_leaves_the_key_out_on_spinel() {
    app().run_spinel_with_db(SCRIPT).assert_passes();
}
