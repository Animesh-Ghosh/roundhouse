//! `pluck(Arel.sql("a, COUNT(*)"))` projects a fragment the caller wrote:
//! it is not a column name to validate, and one literal can stand for
//! several columns. Rails 8.1.4 takes the row width from the result, so
//! one literal naming three columns plucks three-element rows and one
//! naming a single column plucks scalars.
//! The Spinel side is `pluck_sql_literal_spinel.rs`.

#[path = "support/emit_and_run.rs"]
#[allow(dead_code)]
mod emit_and_run;

const APPLICATION_RECORD: &str = "class ApplicationRecord < ActiveRecord::Base\n  primary_abstract_class\nend\n";

fn app() -> emit_and_run::Overlay {
    emit_and_run::empty_app()
        .write(
            "db/schema.rb",
            "ActiveRecord::Schema.define do\n  create_table \"shops\", force: :cascade do |t|\n    t.string \"name\"\n  end\n  create_table \"widgets\", force: :cascade do |t|\n    t.integer \"shop_id\"\n    t.string \"name\"\n    t.integer \"price\"\n  end\nend\n",
        )
        .write("config/routes.rb", "Rails.application.routes.draw do\nend\n")
        .write("app/models/shop.rb", "class Shop < ApplicationRecord\n  has_many :widgets\nend\n")
        .write("app/models/application_record.rb", APPLICATION_RECORD)
        .write(
            "app/models/widget.rb",
            r#"class Widget < ApplicationRecord
  belongs_to :shop, optional: true

  def self.stats
    group(:shop_id).order(:shop_id).pluck(Arel.sql("shop_id, COUNT(*), MIN(price)"))
  end

  def self.counts
    group(:shop_id).order(:shop_id).pluck(Arel.sql("COUNT(*)"))
  end

  def self.mixed
    group(:shop_id).order(:shop_id).pluck(:shop_id, Arel.sql("MAX(price)"))
  end

  def self.same_named
    joins(:shop).order("widgets.id").pluck(Arel.sql("widgets.id, shops.id"))
  end

  def self.aliased
    joins(:shop).order("widgets.id").pluck(Arel.sql("widgets.id AS widget_id, shops.id AS owner_id"))
  end

  def self.plain_and_literal
    joins(:shop).order("widgets.id").pluck(:id, Arel.sql("shops.id"))
  end

  def self.commas_in_expressions
    joins(:shop).order("widgets.id").pluck(Arel.sql("COALESCE(widgets.name, 'a,b'), 1"))
  end
end
"#,
        )
}

const SCRIPT: &str = r#"
Widget.create!(shop_id: 1, price: 5)
Widget.create!(shop_id: 1, price: 3)
Widget.create!(shop_id: 2, price: 7)
got = Widget.stats
raise "stats: #{got.inspect}" unless got == [[1, 2, 3], [2, 1, 7]]
got = Widget.counts
raise "counts: #{got.inspect}" unless got == [2, 1]
got = Widget.mixed
raise "mixed: #{got.inspect}" unless got == [[1, 5], [2, 7]]
puts "pluck literal passed"
"#;

#[test]
fn a_sql_literal_is_projected_raw_and_the_result_gives_the_row_width() {
    app().run_ruby(SCRIPT).assert_passes();
}

const JOIN_SCRIPT: &str = r#"
first = Shop.create!(name: "s")
second = Shop.create!(name: "t")
Widget.create!(shop_id: first.id, price: 5)
Widget.create!(shop_id: first.id, price: 3)
Widget.create!(shop_id: second.id, price: 7)
raised = false
begin
  Widget.same_named
rescue ArgumentError => e
  raised = e.message.include?("share a name")
end
raise "same-named columns did not raise" unless raised
got = Widget.aliased
raise "aliased: #{got.inspect}" unless got == [[1, 1], [2, 1], [3, 2]]
got = Widget.plain_and_literal
raise "plain and literal: #{got.inspect}" unless got == [[1, 1], [2, 1], [3, 2]]
got = Widget.commas_in_expressions
raise "commas: #{got.inspect}" unless got == [["a,b", 1], ["a,b", 1], ["a,b", 1]]
puts "pluck join passed"
"#;

#[test]
fn columns_sharing_a_name_refuse_and_aliases_keep_every_column() {
    app().run_ruby(JOIN_SCRIPT).assert_passes();
}
