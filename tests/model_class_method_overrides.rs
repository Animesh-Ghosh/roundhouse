#[path = "support/emit_and_run.rs"]
mod emit_and_run;

#[test]
fn a_model_class_merge_override_keeps_its_hash_argument() {
    emit_and_run::empty_app()
        .write("db/schema.rb", "ActiveRecord::Schema.define(version: 1) do\n  create_table :widgets do |t|\n    t.string :name\n    t.integer :owner_id\n  end\n  create_table :owners\nend\n")
        .write("app/models/application_record.rb", "class ApplicationRecord < ActiveRecord::Base\nend\n")
        .write("app/models/widget.rb", "class Widget < ApplicationRecord\n  def self.merge(values = {})\n    values[:number].to_i + 1\n  end\nend\n")
        .write("app/models/child_widget.rb", "class ChildWidget < Widget\n  self.table_name = \"widgets\"\nend\n")
        .write("app/models/owner.rb", "class Owner < ApplicationRecord\n  has_many :widgets\n  def matching_widget_count\n    widgets.merge(MergeProbe.selected).count\n  end\nend\n")
        .write("app/models/merge_probe.rb", r#"class MergeProbe
  def self.direct
    Widget.merge(number: 4)
  end

  def self.inherited
    ChildWidget.merge(number: 8)
  end

  def self.relation
    Widget.where(name: "one").merge(Widget.where(name: "one")).count
  end

  def self.selected
    Widget.where(name: "one")
  end
end
"#)
        .run_ruby(r#"
owner = Owner.new
owner.save!
widget = Widget.new
widget.name = "one"
widget.owner_id = owner.id
widget.save!
raise "class merge was rewritten" unless MergeProbe.direct == 5
raise "inherited merge was rewritten" unless MergeProbe.inherited == 9
raise "relation merge changed" unless MergeProbe.relation == 1
raise "association merge changed" unless owner.matching_widget_count == 1
puts "Model class merge overrides retain their arguments; relation merge still queries"
"#)
        .assert_passes();
}

#[test]
fn model_merge_overrides_survive_helpers_branches_and_locals() {
    let script = r#"
raise "helper" unless MergeProbe.helper == 5
raise "if true" unless MergeProbe.choose(true) == 5
raise "if false" unless MergeProbe.choose(false) == 9
raise "case true" unless MergeProbe.choose_case(true) == 5
raise "case false" unless MergeProbe.choose_case(false) == 9
raise "local" unless MergeProbe.local == 5
puts "Model merge helper, branches and local retain ownership"
"#;
    merge_overlay().run_ruby(script).assert_passes();
}

fn merge_overlay() -> emit_and_run::Overlay {
    emit_and_run::empty_app()
        .write("db/schema.rb", "ActiveRecord::Schema.define(version: 1) do\n  create_table :widgets do |t|\n    t.string :name\n  end\nend\n")
        .write("app/models/application_record.rb", "class ApplicationRecord < ActiveRecord::Base\nend\n")
        .write("app/models/widget.rb", "class Widget < ApplicationRecord\n  def self.merge(values = {})\n    values[:number].to_i + 1\n  end\nend\n")
        .write("app/models/child_widget.rb", "class ChildWidget < Widget\n  self.table_name = \"widgets\"\nend\n")
        .write("app/models/merge_probe.rb", r#"class MergeProbe
  def self.helper
    Widget.merge(number: 4)
  end

  def self.choose(flag)
    if flag
      Widget.merge(number: 4)
    else
      ChildWidget.merge(number: 8)
    end
  end

  def self.choose_case(flag)
    case flag
    when true
      Widget.merge(number: 4)
    else
      ChildWidget.merge(number: 8)
    end
  end

  def self.local
    result = Widget.merge(number: 4)
    result
  end
end
"#)
}

#[test]
#[ignore = "requires Spinel"]
fn model_merge_override_runs_natively() {
    merge_overlay().run_spinel_with_rbs(r#"
raise "helper" unless MergeProbe.helper == 5
raise "if true" unless MergeProbe.choose(true) == 5
raise "if false" unless MergeProbe.choose(false) == 9
raise "case true" unless MergeProbe.choose_case(true) == 5
raise "case false" unless MergeProbe.choose_case(false) == 9
raise "local" unless MergeProbe.local == 5
puts "Model merge ownership matches Rails"
"#).assert_passes();
}

#[test]
fn strict_targets_keep_model_merge_ownership() {
    for target in [roundhouse::project::BuildTarget::Rust, roundhouse::project::BuildTarget::Typescript] {
        let (emitted, errors) = merge_overlay().emit(target);
        if target == roundhouse::project::BuildTarget::Rust {
            assert_eq!(errors.len(), 1, "{errors:?}");
            assert!(errors[0].contains("plain Ruby class not supported (rust): class `MergeProbe` is not emitted for this target (the ruby and typescript emits carry it)"), "{errors:?}");
            continue;
        }
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("Case not supported (typescript)"), "{errors:?}");
        let path = "app/models/merge_probe.ts";
        let source = std::fs::read_to_string(emitted.join(path)).expect("emitted probe");
        assert!(!source.contains("Relation::new"), "{source}");
        assert!(!source.contains("new Relation"), "{source}");
    }
}
