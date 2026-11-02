use super::{class_body_calls_contract as contract, emit_and_run};
use roundhouse::project::BuildTarget;

#[test]
fn ordinary_class_body_calls_keep_receiver_state_and_order() {
    contract::overlay().run_ruby(contract::ASSERTIONS).assert_passes();
}

#[test]
fn ordinary_class_body_calls_are_refused_without_load_time_execution() {
    let (_emitted, errors) = contract::overlay().emit(BuildTarget::Python);
    assert!(errors.iter().any(|e| e.contains("class body")), "{errors:#?}");
}

#[test]
fn instance_only_method_is_not_a_class_body_target() {
    let (tree, _errors) = emit_and_run::real_blog()
        .write("app/models/instance_only.rb", "class InstanceOnly\n def label(value) = value\nend\n")
        .write("app/models/instance_child.rb", "class InstanceChild < InstanceOnly\n label \"missing\"\nend\n")
        .emit(BuildTarget::Spinel);
    let text = std::fs::read_to_string(tree.join("app/models/instance_child.rb")).expect("child emitted");
    assert!(!text.contains("\"missing\""), "{text}");
}

#[test]
fn own_override_and_computed_argument_run_once_in_source_order() {
    contract::overlay()
        .write("app/models/labelled_document.rb", r#"
class LabelledDocument < DocumentBase
  def self.label(value = nil)
    value ? @label = value.upcase : @label
  end
  self.label "sam" + "ple"
  def self.value = label
end
"#)
        .run_ruby("raise LabelledDocument.value.inspect unless LabelledDocument.value == \"SAMPLE\"\nputs \"override passed\"")
        .assert_passes();
}

#[test]
fn a_namespaced_rooted_parent_keeps_its_own_receiver() {
    contract::overlay()
        .write("app/models/namespaced_document.rb", r#"
module Envelope
  class DocumentBase
    def self.label(value = nil)
      value ? @label = value : @label
    end
  end
  class Document < ::DocumentBase
    label "rooted"
  end
  class Nested < DocumentBase
    label "nested"
  end
end
"#)
        .run_ruby("raise unless Envelope::Document.label == \"rooted\"\nraise unless Envelope::Nested.label == \"nested\"\nraise unless DocumentBase.label.nil?\nputs \"namespaces passed\"")
        .assert_passes();
}

#[test]
fn a_runtime_defining_extended_method_is_refused_by_spinel() {
    let (_tree, errors) = contract::overlay()
        .write("app/models/label_methods.rb", r#"
module LabelMethods
  def label(name)
    define_singleton_method(name) { "runtime" }
  end
end
"#)
        .emit(BuildTarget::Spinel);
    assert!(errors.iter().any(|e| e.contains("runtime-defined method")), "{errors:#?}");
}

#[test]
fn an_unresolved_call_stays_reported_and_unreplayed() {
    let (tree, _) = contract::overlay()
        .write("app/models/labelled_document.rb", "class LabelledDocument < DocumentBase\n unproven \"missing\"\nend\n")
        .emit(BuildTarget::Spinel);
    let text = std::fs::read_to_string(tree.join("app/models/labelled_document.rb")).unwrap();
    assert!(!text.contains("\"missing\""), "{text}");
}

#[test]
fn each_non_ruby_target_refuses_the_class_body_call() {
    for &target in BuildTarget::TRANSPILE {
        if matches!(target, BuildTarget::Ruby | BuildTarget::Jruby | BuildTarget::Spinel | BuildTarget::Roda | BuildTarget::Futamura) {
            continue;
        }
        let (_, errors) = contract::overlay().emit(target);
        assert!(errors.iter().any(|e| e.contains("class body")), "{target:?}: {errors:#?}");
    }
}

#[test]
fn a_late_override_is_refused_before_spinel_changes_the_earlier_call() {
    let (_, errors) = contract::overlay()
        .write("app/models/document_base.rb", r#"
class DocumentBase
  def self.label(value = nil)
    value ? @label = decorate(value) : @label
  end
  def self.decorate(value) = "parent:" + value
end
"#)
        .write("app/models/labelled_document.rb", r#"
class LabelledDocument < DocumentBase
  label "sample"
  def self.decorate(value) = "child:" + value
  def self.value = label
end
"#)
        .emit(BuildTarget::Spinel);
    assert!(errors.iter().any(|e| e.contains("class-body method binding")), "{errors:#?}");
}

#[test]
fn a_redefined_ancestor_is_refused_on_spinel() {
    let (_, errors) = contract::overlay()
        .write("app/models/document_reopen.rb", r#"
class DocumentBase
  def self.label(value = nil)
    value ? @label = "new:" + value : @label
  end
end
"#)
        .emit(BuildTarget::Spinel);
    assert!(errors.iter().any(|e| e.contains("class-body method binding")), "{errors:#?}");
}

#[test]
fn redefined_ancestors_are_refused_on_ruby_as_well() {
    for target in [BuildTarget::Ruby, BuildTarget::Jruby] {
        let (_, errors) = contract::overlay()
            .write("app/models/document_reopen.rb", "class DocumentBase\n def self.label(value = nil)\n value ? @label = \"later:\" + value : @label\n end\nend\n")
            .emit(target);
        assert!(errors.iter().any(|e| e.contains("class-body method binding")), "{target:?}: {errors:#?}");
    }
}

#[test]
fn an_inherited_call_does_not_cross_an_unplaced_extension_call() {
    let (_, errors) = contract::overlay()
        .write("app/models/prior_settings.rb", "module PriorSettings\n def audit = label\nend\n")
        .write("app/models/labelled_document.rb", "class LabelledDocument < DocumentBase\n extend PriorSettings\n audit\n label \"sample\"\nend\n")
        .emit(BuildTarget::Ruby);
    assert!(errors.iter().any(|e| e.contains("class-body method binding")), "{errors:#?}");
}
