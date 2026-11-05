//! Callback registration order, compared with the same declarations on Rails.
#[path = "support/emit_and_run.rs"]
mod emit_and_run;

const BASE: &str =
    "class ApplicationRecord < ActiveRecord::Base\n  self.abstract_class = true\nend\n";
const SCHEMA: &str = r#"ActiveRecord::Schema.define do
  create_table :items, force: true do |t|
    t.string :name
    t.string :code
    t.string :type
    t.boolean :enabled
  end
end
"#;

fn app(files: &[(&str, &str)]) -> emit_and_run::Overlay {
    let mut app = emit_and_run::empty_app()
        .write("app/models/application_record.rb", BASE)
        .write("db/schema.rb", SCHEMA);
    for (path, source) in files {
        app = app.write(path, source);
    }
    app
}

fn compare(files: &[(&str, &str)], script: &str) {
    // Scheduling has no application config dependency: use standalone AR,
    // with its default settings, rather than a Rails application/load_defaults.
    let oracle = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/support/rails_oracle.rb");
    let sources = files
        .iter()
        .map(|(_, source)| *source)
        .collect::<Vec<_>>()
        .join("\n");
    let output = std::process::Command::new("ruby")
        .args([
            "-e",
            &format!(
                r#"require {oracle:?}
unless RailsOracle.available?("activerecord", sqlite3: ">= 2.9")
  warn "skipping Rails control: #{{RailsOracle.label}} and sqlite3 >= 2.9 are required"
  exit 77
end
RailsOracle.activate("activerecord", sqlite3: ">= 2.9")
require "active_record"
ActiveRecord::Base.establish_connection(adapter: "sqlite3", database: ":memory:")
ActiveRecord::Schema.verbose = false
{SCHEMA}
{BASE}
{sources}
{script}"#
            ),
        ])
        .output()
        .expect("Rails oracle");
    if output.status.code() == Some(77) {
        eprintln!("{}", String::from_utf8_lossy(&output.stderr));
    } else {
        assert!(
            output.status.success(),
            "Rails: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let run = app(files).run_ruby(script);
    run.assert_passes();
    if output.status.success() {
        assert_eq!(
            run.stdout.trim(),
            String::from_utf8_lossy(&output.stdout).trim(),
            "declarations: {sources}"
        );
    }
}

const VALIDITY_SCRIPT: &str = r#"record = Item.new(name: "", code: "", enabled: true)
puts record.valid?
puts record.errors.to_a.join("|")
"#;

const FILL_VALIDATOR: &str = r#"class FillValidator < ActiveModel::EachValidator
  def validate_each(record, attribute, value)
    record.name = "ok"
  end
end
"#;

const MIXED_RULES: &str = r#"class Item < ApplicationRecord
  validates :name, fill: { prepend: true }, presence: { prepend: true }
end
"#;

#[test]
fn custom_and_builtin_rules_keep_registration_order() {
    compare(
        &[
            ("app/validators/fill_validator.rb", FILL_VALIDATOR),
            ("app/models/item.rb", MIXED_RULES),
        ],
        VALIDITY_SCRIPT,
    );
}

const REPEATED_METHOD: &str = r#"class Item < ApplicationRecord
  validate :check_once
  validate :check_once, prepend: true
  def check_once
    self.enabled = !enabled
    errors.add(:base, "ran twice") if enabled
  end
end
"#;

#[test]
fn a_symbol_registration_replaces_its_previous_callback() {
    compare(&[("app/models/item.rb", REPEATED_METHOD)], VALIDITY_SCRIPT);
}

const OWNED_VALIDATE: &str = r#"class Item < ApplicationRecord
  def self.validate(*args, **options)
  end
  validate :a, prepend: true
  def a
    errors.add(:base, "a")
  end
end
"#;

#[test]
fn an_app_owned_class_method_is_not_the_validation_dsl() {
    compare(&[("app/models/item.rb", OWNED_VALIDATE)], VALIDITY_SCRIPT);
}

const OWNED_MULTI_VALIDATE: &str = r#"class Item < ApplicationRecord
  CALLS = []
  def self.validate(*names)
    CALLS << names.join(",")
  end
  def self.mark(value)
    CALLS << value
  end
  mark "before"
  # Keep the batch comment on its one call.
  validate :a, :b
  # Keep the following body item after the call.
  mark "after"
end
"#;

const OWNED_MULTI_SCRIPT: &str = r#"puts Item::CALLS.join("|")
raise Item::CALLS.inspect unless Item::CALLS == ["before", "a,b", "after"]
"#;

#[test]
fn an_app_owned_validate_receives_a_symbol_batch_once() {
    compare(&[("app/models/item.rb", OWNED_MULTI_VALIDATE)], OWNED_MULTI_SCRIPT);
    let (_, ingested, errors) = app(&[("app/models/item.rb", OWNED_MULTI_VALIDATE)])
        .emit_with_app(roundhouse::project::BuildTarget::Ruby);
    assert!(errors.is_empty(), "{errors:?}");
    let model = ingested.models.iter().find(|model| model.name.0.as_str() == "Item").unwrap();
    let calls: Vec<_> = model.body.iter().filter_map(|item| {
        let roundhouse::dialect::ModelBodyItem::Unknown { expr, leading_comments, .. } = item else { return None };
        let roundhouse::expr::ExprNode::Send { method, .. } = &*expr.node else { return None };
        matches!(method.as_str(), "mark" | "validate").then_some((method.as_str(), leading_comments))
    }).collect();
    assert_eq!(calls.iter().map(|(name, _)| *name).collect::<Vec<_>>(), ["mark", "validate", "mark"]);
    assert_eq!(calls[1].1.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(),
        ["# Keep the batch comment on its one call."]);
    assert_eq!(calls[2].1.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(),
        ["# Keep the following body item after the call."]);
}

#[test]
fn distinct_app_owned_validation_calls_keep_their_order() {
    let model = OWNED_MULTI_VALIDATE.replace(
        "  mark \"after\"",
        "  mark \"between\"\n  validate :a, :b\n  mark \"after\"",
    );
    compare(
        &[("app/models/item.rb", &model)],
        "puts Item::CALLS.join(\"|\")\nraise Item::CALLS.inspect unless Item::CALLS == [\"before\", \"a,b\", \"between\", \"a,b\", \"after\"]\n",
    );
}

const NIL_PREPEND: &str = r#"class Item < ApplicationRecord
  validates :name, presence: { prepend: nil }
end
"#;

#[test]
fn nil_prepend_keeps_the_validation() {
    compare(&[("app/models/item.rb", NIL_PREPEND)], VALIDITY_SCRIPT);
}

#[test]
fn mixed_rule_permutations_keep_each_registration_in_place() {
    for rules in [
        "presence: { prepend: true }, fill: { prepend: true }",
        "fill: { prepend: true }, presence: true",
        "fill: true, presence: { prepend: true }",
        "fill: true, presence: true",
    ] {
        let model = format!("class Item < ApplicationRecord\n  validates :name, {rules}\nend\n");
        compare(
            &[
                ("app/validators/fill_validator.rb", FILL_VALIDATOR),
                ("app/models/item.rb", &model),
            ],
            VALIDITY_SCRIPT,
        );
    }
}

#[test]
fn symbol_identity_is_independent_of_position_and_conditions() {
    for declarations in [
        "validate :a, :b, :a, prepend: true",
        "validate :a, prepend: true\n  validate :b\n  validate :a",
        "validate :a\n  validate :a, prepend: true, if: -> { false }",
        "validate :a, :b\n  validate :a, prepend: true",
    ] {
        let model = format!(
            r#"class Item < ApplicationRecord
  {declarations}
  def a
    errors.add(:base, "a")
  end
  def b
    errors.add(:base, "b")
  end
end
"#
        );
        compare(&[("app/models/item.rb", &model)], VALIDITY_SCRIPT);
    }
}

#[test]
fn class_method_ownership_respects_ancestors_concerns_and_definition_order() {
    let callback = "validate :a, prepend: true\n  def a\n    errors.add(:base, 'a')\n  end\n";
    let override_method = "def self.validate(*args, **options)\n  end\n";
    let parent = format!(
        "class Parent < ApplicationRecord\n  self.abstract_class = true\n  {override_method}\nend\n"
    );
    let child = format!("class Item < Parent\n  {callback}\nend\n");
    compare(
        &[
            ("app/models/parent.rb", &parent),
            ("app/models/item.rb", &child),
        ],
        VALIDITY_SCRIPT,
    );
    let late = format!("class Item < ApplicationRecord\n  {callback}\n  {override_method}\nend\n");
    compare(&[("app/models/item.rb", &late)], VALIDITY_SCRIPT);
    let instance = format!(
        "class Item < ApplicationRecord\n  def validate(*args)\n  end\n  {callback}\nend\n"
    );
    compare(&[("app/models/item.rb", &instance)], VALIDITY_SCRIPT);
    let concern = r#"module ValidationOwner
  extend ActiveSupport::Concern
  class_methods do
    def validate(*args, **options)
    end
  end
end
"#;
    for body in [
        format!("include ValidationOwner\n  {callback}"),
        format!("{callback}\n  include ValidationOwner"),
    ] {
        let model = format!("class Item < ApplicationRecord\n  {body}\nend\n");
        compare(
            &[
                ("app/models/concerns/validation_owner.rb", concern),
                ("app/models/item.rb", &model),
            ],
            VALIDITY_SCRIPT,
        );
    }
}

#[test]
fn scheduling_uses_ruby_truthiness_for_literal_and_constant_options() {
    for value in [
        "nil",
        "false",
        "0",
        "3.5",
        "''",
        ":first",
        "[]",
        "{}",
        "{ enabled: false }",
        "FLAG",
    ] {
        let model = format!(
            r#"class Item < ApplicationRecord
  FLAG = false
  validates :code, presence: true
  validates :name, presence: {{ prepend: {value} }}
end
"#
        );
        compare(&[("app/models/item.rb", &model)], VALIDITY_SCRIPT);
    }
    for value in ["nil", "0"] {
        let model = format!(
            "class Item < ApplicationRecord\n  validates :name, presence: true\n  validates :name, fill: {{ prepend: {value} }}\nend\n"
        );
        compare(
            &[
                ("app/validators/fill_validator.rb", FILL_VALIDATOR),
                ("app/models/item.rb", &model),
            ],
            VALIDITY_SCRIPT,
        );
    }
}

#[test]
fn an_sti_reregistration_replaces_only_the_childs_callback() {
    let child = r#"class SpecialItem < Item
  validate :check_once, prepend: true
end
"#;
    compare(
        &[
            ("app/models/item.rb", REPEATED_METHOD),
            ("app/models/special_item.rb", child),
        ],
        r#"[Item, SpecialItem].each do |klass|
  record = klass.new(enabled: true)
  puts record.valid?
  puts record.errors.to_a.join("|")
end
"#,
    );
}

const EMPTY_ITEM: &str = "class Item < ApplicationRecord\nend\n";
const OWNED_CHILD: &str = r#"class SpecialItem < Item
  def self.validate(*args, **options)
  end
  validate :a, prepend: true
  def a
    errors.add(:base, "a")
  end
end
"#;
const CHILD_SCRIPT: &str =
    "record = SpecialItem.new\nputs record.valid?\nputs record.errors.to_a.join('|')\n";

#[test]
fn ownership_on_an_sti_child_is_respected() {
    compare(
        &[
            ("app/models/item.rb", EMPTY_ITEM),
            ("app/models/special_item.rb", OWNED_CHILD),
        ],
        CHILD_SCRIPT,
    );
}

const OWNED_PARENT: &str =
    "class Item < ApplicationRecord\n  def self.validate(*args, **options)\n  end\nend\n";
const INHERITED_CHILD: &str = r#"class SpecialItem < Item
  validate :a, prepend: true
  def a
    errors.add(:base, "a")
  end
end
"#;

#[test]
fn ownership_inherited_by_an_sti_child_is_respected() {
    compare(
        &[
            ("app/models/item.rb", OWNED_PARENT),
            ("app/models/special_item.rb", INHERITED_CHILD),
        ],
        CHILD_SCRIPT,
    );
}

const EXTENDED_OWNER: &str =
    "module ValidationOwner\n  def validate(*args, **options)\n  end\nend\n";
const EXTENDING_ITEM: &str = r#"class Item < ApplicationRecord
  extend ValidationOwner
  validate :a, prepend: true
  def a
    errors.add(:base, "a")
  end
end
"#;

#[test]
fn ownership_supplied_by_an_extended_module_is_respected() {
    compare(
        &[
            ("app/models/concerns/validation_owner.rb", EXTENDED_OWNER),
            ("app/models/item.rb", EXTENDING_ITEM),
        ],
        VALIDITY_SCRIPT,
    );
}

const FLAG_EACH_VALIDATOR: &str = r#"class FlagValidator < ActiveModel::EachValidator
  def validate_each(record, attribute, value)
    record.errors.add(:base, "false option") if options[:prepend] == false
  end
end
"#;
const FLAG_WITH_VALIDATOR: &str = r#"class FlagValidator < ActiveModel::Validator
  def validate(record)
    record.errors.add(:base, "false option") if options[:prepend] == false
  end
end
"#;
const FLAG_EACH_ITEM: &str = "class Item < ApplicationRecord\n  FLAG = false\n  validates :name, flag: { prepend: FLAG }\nend\n";
const FLAG_WITH_ITEM: &str = "class Item < ApplicationRecord\n  FLAG = false\n  validates_with FlagValidator, prepend: FLAG\nend\n";

#[test]
fn constant_prepend_on_an_each_validator_keeps_its_original_value() {
    compare(
        &[
            ("app/validators/flag_validator.rb", FLAG_EACH_VALIDATOR),
            ("app/models/item.rb", FLAG_EACH_ITEM),
        ],
        VALIDITY_SCRIPT,
    );
}

#[test]
fn constant_prepend_on_validates_with_keeps_its_original_value() {
    compare(
        &[
            ("app/validators/flag_validator.rb", FLAG_WITH_VALIDATOR),
            ("app/models/item.rb", FLAG_WITH_ITEM),
        ],
        VALIDITY_SCRIPT,
    );
}

#[test]
#[ignore = "requires the Spinel toolchain"]
fn registration_order_identity_ownership_and_truthiness_run_natively() {
    app(&[("app/models/item.rb", OWNED_MULTI_VALIDATE)])
        .run_spinel_with_db(OWNED_MULTI_SCRIPT)
        .assert_passes();
    let cases: &[(&[(&str, &str)], &str, bool, &str)] = &[
        (
            &[
                ("app/validators/fill_validator.rb", FILL_VALIDATOR),
                ("app/models/item.rb", MIXED_RULES),
            ],
            "Item",
            false,
            "Name can't be blank",
        ),
        (&[("app/models/item.rb", REPEATED_METHOD)], "Item", true, ""),
        (&[("app/models/item.rb", OWNED_VALIDATE)], "Item", true, ""),
        (
            &[("app/models/item.rb", NIL_PREPEND)],
            "Item",
            false,
            "Name can't be blank",
        ),
        (
            &[
                ("app/models/item.rb", EMPTY_ITEM),
                ("app/models/special_item.rb", OWNED_CHILD),
            ],
            "SpecialItem",
            true,
            "",
        ),
        (
            &[
                ("app/models/item.rb", OWNED_PARENT),
                ("app/models/special_item.rb", INHERITED_CHILD),
            ],
            "SpecialItem",
            true,
            "",
        ),
        (
            &[
                ("app/models/concerns/validation_owner.rb", EXTENDED_OWNER),
                ("app/models/item.rb", EXTENDING_ITEM),
            ],
            "Item",
            true,
            "",
        ),
        (
            &[
                ("app/validators/flag_validator.rb", FLAG_EACH_VALIDATOR),
                ("app/models/item.rb", FLAG_EACH_ITEM),
            ],
            "Item",
            false,
            "false option",
        ),
        (
            &[
                ("app/validators/flag_validator.rb", FLAG_WITH_VALIDATOR),
                ("app/models/item.rb", FLAG_WITH_ITEM),
            ],
            "Item",
            false,
            "false option",
        ),
    ];
    for (files, class, valid, errors) in cases {
        let script = format!(
            r#"record = {class}.new(name: "", code: "", enabled: true)
raise "validity" unless record.valid? == {valid}
raise record.errors.to_a.inspect unless record.errors.to_a.join("|") == {errors:?}
"#
        );
        app(files).run_spinel_with_db(&script).assert_passes();
    }
}

const METHODS: &str = r#"class Item < ApplicationRecord
  validate :tail
  validate :a, :b, prepend: true
  validate :c, prepend: true
  def tail
    errors.add(:base, "tail")
  end
  def a
    errors.add(:base, "a")
  end
  def b
    errors.add(:base, "b")
  end
  def c
    errors.add(:base, "c")
  end
end
"#;
const METHODS_SCRIPT: &str = r#"record = Item.new
record.valid?
raise record.errors.to_a.inspect unless record.errors.to_a == ["c", "b", "a", "tail"]
puts record.errors.to_a.join("|")
"#;

#[test]
fn each_prepended_method_is_a_callback() {
    compare(&[("app/models/item.rb", METHODS)], METHODS_SCRIPT);
}

fn rule_case(rule: &str) {
    let source = format!(
        r#"class Item < ApplicationRecord
  validate :tail
  validates :name, {rule}
  def tail
    errors.add(:base, "tail")
  end
end
"#
    );
    compare(
        &[("app/models/item.rb", &source)],
        r#"record = Item.new(name: "")
record.valid?
raise record.errors.to_a.inspect unless record.errors.to_a == ["Name first", "tail"]
puts record.errors.to_a.join("|")
"#,
    );
}

#[test]
fn presence_reads_the_scheduling_option() {
    rule_case("presence: { prepend: true, message: 'first' }");
}

#[test]
fn length_reads_the_scheduling_option() {
    rule_case("length: { minimum: 2, prepend: true, message: 'first' }");
}

#[test]
fn numericality_reads_the_scheduling_option() {
    rule_case("numericality: { prepend: true, message: 'first' }");
}

#[test]
fn format_reads_the_scheduling_option() {
    rule_case("format: { with: /\\Aok\\z/, prepend: true, message: 'first' }");
}

#[test]
fn inclusion_exclusion_and_acceptance_read_the_scheduling_option() {
    for rule in [
        "inclusion: { in: ['ok'], prepend: true, message: 'first' }",
        "exclusion: { in: [''], prepend: true, message: 'first' }",
        "acceptance: { prepend: true, message: 'first' }",
    ] {
        rule_case(rule);
    }
}

#[test]
fn absence_reads_the_scheduling_option() {
    compare(
        &[(
            "app/models/item.rb",
            r#"class Item < ApplicationRecord
  validates :code, presence: true
  validates :name, absence: { prepend: true }
end
"#,
        )],
        r#"record = Item.new(name: "present")
record.valid?
raise record.errors.to_a.inspect unless record.errors.to_a == ["Name must be blank", "Code can't be blank"]
puts record.errors.to_a.join("|")
"#,
    );
}

#[test]
fn uniqueness_reads_the_scheduling_option() {
    compare(
        &[(
            "app/models/item.rb",
            r#"class Item < ApplicationRecord
  validates :code, presence: true
  validates :name, uniqueness: { prepend: true }
end
"#,
        )],
        r#"Item.create!(name: "taken", code: "ok")
record = Item.new(name: "taken")
record.valid?
raise record.errors.to_a.inspect unless record.errors.to_a == ["Name has already been taken", "Code can't be blank"]
puts record.errors.to_a.join("|")
"#,
    );
}

#[test]
fn a_custom_each_validator_reads_the_scheduling_option() {
    compare(
        &[
            (
                "app/validators/first_validator.rb",
                r#"class FirstValidator < ActiveModel::EachValidator
  def validate_each(record, attribute, value)
    record.errors.add(attribute, "first") if options[:prepend]
  end
end
"#,
            ),
            (
                "app/models/item.rb",
                r#"class Item < ApplicationRecord
  validates :name, presence: true
  validates :code, first: { prepend: true }
end
"#,
            ),
        ],
        r#"record = Item.new
record.valid?
raise record.errors.to_a.inspect unless record.errors.to_a == ["Code first", "Name can't be blank"]
puts record.errors.to_a.join("|")
"#,
    );
}

#[test]
fn a_validator_named_prepend_keeps_its_rule_name() {
    compare(
        &[
            (
                "app/validators/prepend_validator.rb",
                r#"class PrependValidator < ActiveModel::EachValidator
  def validate_each(record, attribute, value)
    record.errors.add(attribute, "validator")
  end
end
"#,
            ),
            (
                "app/models/item.rb",
                r#"class Item < ApplicationRecord
  validates :name, presence: true
  validates :code, prepend: true
end
"#,
            ),
        ],
        r#"record = Item.new
record.valid?
raise record.errors.to_a.inspect unless record.errors.to_a == ["Name can't be blank", "Code validator"]
puts record.errors.to_a.join("|")
"#,
    );
}

#[test]
fn the_legacy_helper_and_positional_rule_hash_read_the_scheduling_option() {
    compare(
        &[(
            "app/models/item.rb",
            r#"class Item < ApplicationRecord
  validates :name, presence: true
  validates_numericality_of :code, prepend: true
  validates :code, { length: { minimum: 2, prepend: true } }
end
"#,
        )],
        r#"record = Item.new(code: "")
record.valid?
want = ["Code is too short (minimum is 2 characters)", "Code is not a number", "Name can't be blank"]
raise record.errors.to_a.inspect unless record.errors.to_a == want
puts record.errors.to_a.join("|")
"#,
    );
}

const VALIDATOR: &str = r#"class FirstValidator < ActiveModel::Validator
  def validate(record)
    record.errors.add(:base, "validator")
    record.errors.add(:base, "missing prepend option") unless options[:prepend]
  end
end
"#;

#[test]
fn validates_with_passes_scheduling_to_the_callback() {
    compare(
        &[
            ("app/validators/first_validator.rb", VALIDATOR),
            (
                "app/models/item.rb",
                r#"class Item < ApplicationRecord
  validates :name, presence: true
  validates_with FirstValidator, prepend: true
end
"#,
            ),
        ],
        r#"record = Item.new
record.valid?
raise record.errors.to_a.inspect unless record.errors.to_a == ["validator", "Name can't be blank"]
puts record.errors.to_a.join("|")
"#,
    );
}

#[test]
fn an_sti_child_prepends_over_its_parent() {
    compare(
        &[
            (
                "app/models/item.rb",
                "class Item < ApplicationRecord\n  validates :name, presence: true\nend\n",
            ),
            (
                "app/models/special_item.rb",
                r#"class SpecialItem < Item
  validates :code, presence: { prepend: true }
end
"#,
            ),
        ],
        r#"child = SpecialItem.new
child.valid?
raise child.errors.to_a.inspect unless child.errors.to_a == ["Code can't be blank", "Name can't be blank"]
parent = Item.new
parent.valid?
raise parent.errors.to_a.inspect unless parent.errors.to_a == ["Name can't be blank"]
puts child.errors.to_a.join("|")
puts parent.errors.to_a.join("|")
"#,
    );
}

#[test]
fn prepend_combines_with_context_and_predicate() {
    compare(
        &[(
            "app/models/item.rb",
            r#"class Item < ApplicationRecord
  validates :name, presence: true
  validates :code, presence: { prepend: true, on: :create, if: :enabled? }
end
"#,
        )],
        r#"record = Item.new(enabled: true)
record.valid?
raise record.errors.to_a.inspect unless record.errors.to_a == ["Code can't be blank", "Name can't be blank"]
record.enabled = false
record.valid?
raise record.errors.to_a.inspect unless record.errors.to_a == ["Name can't be blank"]
record.name = "ok"
record.code = "ok"
record.save!
record.name = nil
record.code = nil
record.enabled = true
record.valid?
raise record.errors.to_a.inspect unless record.errors.to_a == ["Name can't be blank"]
puts record.errors.to_a.join("|")
"#,
    );
}

#[test]
fn a_rule_keeps_its_attributes_together_and_false_keeps_declaration_order() {
    compare(
        &[(
            "app/models/item.rb",
            r#"class Item < ApplicationRecord
  validates :name, presence: { message: "tail" }
  validates :name, :code, presence: { prepend: true, message: "presence" },
    length: { minimum: 2, prepend: true, message: "length" }
  validates :name, format: { with: /\Aok\z/, prepend: false, message: "last" }
end
"#,
        )],
        r#"record = Item.new(name: "", code: "")
record.valid?
want = ["Name length", "Code length", "Name presence", "Code presence", "Name tail", "Name last"]
raise record.errors.to_a.inspect unless record.errors.to_a == want
puts record.errors.to_a.join("|")
"#,
    );
}

#[test]
#[ignore = "requires the Spinel toolchain"]
fn prepended_callbacks_run_natively() {
    app(&[("app/models/item.rb", METHODS)])
        .run_spinel_with_db(METHODS_SCRIPT)
        .assert_passes();
}

const NATIVE_ITEM: &str = r#"class Item < ApplicationRecord
  validates :name, presence: { message: "ordinary" }
  validate :tail
  validate :a, :b, prepend: true
  validates :code, presence: { prepend: true, message: "presence" },
    length: { minimum: 2, prepend: true, message: "length" },
    numericality: { prepend: true, message: "numericality" },
    format: { with: /\Aok\z/, prepend: true, message: "format" }
  validates_with FirstValidator, prepend: true
  def a
    errors.add(:base, "a")
  end
  def b
    errors.add(:base, "b")
  end
  def tail
    errors.add(:base, "tail")
  end
end
"#;
const NATIVE_CHILD: &str = r#"class SpecialItem < Item
  validates :name, presence: { prepend: true, on: :create, if: :enabled?, message: "child" }
  def a
    errors.add(:base, "child a")
  end
end
"#;
const NATIVE_FILES: &[(&str, &str)] = &[
    ("app/validators/first_validator.rb", VALIDATOR),
    ("app/models/item.rb", NATIVE_ITEM),
    ("app/models/special_item.rb", NATIVE_CHILD),
];
const NATIVE_SCRIPT: &str = r#"record = SpecialItem.new(name: "", code: "", enabled: true)
record.valid?
want = ["Name child", "validator", "Code format", "Code numericality", "Code length", "Code presence", "b", "child a", "Name ordinary", "tail"]
raise record.errors.to_a.inspect unless record.errors.to_a == want
puts record.errors.to_a.join("|")
record.enabled = false
record.valid?
want.shift
raise record.errors.to_a.inspect unless record.errors.to_a == want
puts record.errors.to_a.join("|")
"#;

#[test]
fn native_expected_order_is_checked_by_rails() {
    compare(NATIVE_FILES, NATIVE_SCRIPT);
}

#[test]
#[ignore = "requires the Spinel toolchain"]
fn rules_validators_inheritance_and_guards_run_natively() {
    app(NATIVE_FILES)
        .run_spinel_with_db(NATIVE_SCRIPT)
        .assert_passes();
}

#[test]
fn the_default_schedule_does_not_change_serialized_validations() {
    let validation = roundhouse::dialect::Validation {
        prepend: false,
        attribute: "name".into(),
        rules: vec![roundhouse::dialect::ValidationRule::Presence],
        if_cond: None,
        unless_cond: None,
        message: None,
    };
    let value = serde_json::to_value(&validation).unwrap();
    assert!(value.get("prepend").is_none());
    assert_eq!(
        serde_json::from_value::<roundhouse::dialect::Validation>(value).unwrap(),
        validation
    );
}

#[test]
fn roda_keeps_prepended_presence_in_callback_order() {
    let tree = [
        ("app/models/application_record.rb", BASE),
        ("db/schema.rb", SCHEMA),
        (
            "app/models/item.rb",
            r#"class Item < ApplicationRecord
  validates :name, presence: { message: "ordinary" }
  validates :name, length: { minimum: 2, prepend: true, message: "length" }
  validates :name, presence: { prepend: true, message: "first" }
end
"#,
        ),
    ]
    .into_iter()
    .map(|(path, source)| (std::path::PathBuf::from(path), source.as_bytes().to_vec()))
    .collect();
    let app = roundhouse::ingest::ingest_app_from_tree(tree).unwrap();
    let files = roundhouse::emit::roda::emit(&app);
    let model = &files
        .iter()
        .find(|f| f.path.ends_with("models/item.rb"))
        .expect("Roda model")
        .content;
    let output = std::process::Command::new("ruby")
        .args([
            "-e",
            &format!(
                r#"unless Gem::Specification.find_all_by_name("sequel").any?
  warn "skipping Roda control: sequel is required"
  exit 77
end
require "sequel"
DB = Sequel.sqlite
DB.create_table(:items) {{ primary_key :id; String :name }}
Sequel::Model.plugin :validation_helpers
{model}
record = Item.new(name: "")
record.valid?
raise record.errors.inspect unless record.errors[:name] == ["first", "length", "ordinary"]
"#
            ),
        ])
        .output()
        .expect("Sequel control");
    if output.status.code() == Some(77) {
        eprintln!("{}", String::from_utf8_lossy(&output.stderr));
    } else {
        assert!(
            output.status.success(),
            "Sequel: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
