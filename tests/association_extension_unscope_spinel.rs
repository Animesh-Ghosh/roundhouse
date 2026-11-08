//! The Spinel side of `tests/association_extension_unscope.rs`. Needs a
//! Spinel compiler, so `#[ignore]`d; CI's `spinel-framework` job runs it
//! (`scripts/ci-plan.py`'s `SPINEL_TESTS`).

#[path = "support/emit_and_run.rs"]
#[allow(dead_code)]
mod emit_and_run;

const APPLICATION_RECORD: &str = "class ApplicationRecord < ActiveRecord::Base\n  primary_abstract_class\nend\n";

fn app() -> emit_and_run::Overlay {
    emit_and_run::empty_app()
        .write(
            "db/schema.rb",
            "ActiveRecord::Schema.define do\n  create_table \"shops\", force: :cascade do |t|\n    t.string \"name\"\n  end\n  create_table \"widgets\", force: :cascade do |t|\n    t.integer \"shop_id\"\n    t.string \"name\"\n    t.boolean \"active\", default: true, null: false\n  end\nend\n",
        )
        .write("config/routes.rb", "Rails.application.routes.draw do\nend\n")
        .write("app/models/application_record.rb", APPLICATION_RECORD)
        .write("app/models/widget.rb", "class Widget < ApplicationRecord\n  belongs_to :shop, optional: true\nend\n")
        .write(
            "app/models/shop.rb",
            r#"class Shop < ApplicationRecord
  has_many :widgets, -> { where(active: true) } do
    def inactive = unscope(:where).where(active: false)
    def lookup(id) = find(id)
    def names_only = select(:name)
  end

  def inactive_names
    self.widgets.inactive.order(:name).map { |w| w.name }
  end

  def only_names
    self.widgets.names_only.map { |w| w.name }
  end

  def lookup_name(id)
    self.widgets.lookup(id).name
  end
end
"#,
        )
}

const SCRIPT: &str = r#"
s = Shop.create!(name: "s")
t = Shop.create!(name: "t")
a = Widget.create!(shop_id: s.id, name: "a")
Widget.create!(shop_id: s.id, name: "b", active: false)
Widget.create!(shop_id: t.id, name: "c")
d = Widget.create!(shop_id: t.id, name: "d", active: false)
got = s.inactive_names
raise "inactive: #{got.inspect}" unless got == ["b", "d"]
got = s.only_names
raise "select: #{got.inspect}" unless got == ["a"]
raise "find: #{s.lookup_name(a.id)}" unless s.lookup_name(a.id) == "a"
begin
  s.lookup_name(d.id)
  raise "find crossed the association"
rescue ActiveRecord::RecordNotFound
end
puts "extension unscope passed"
"#;

#[test]
#[ignore = "requires the Spinel toolchain"]
fn bare_unscope_and_find_in_an_extension_reach_the_association_on_spinel() {
    app().run_spinel_with_db(SCRIPT).assert_passes();
}
