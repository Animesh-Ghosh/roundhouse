//! Warden's `Strategies.add(label, strategy = nil)`: `strategy ||=
//! Class.new(Base)` keeps a parameter that callers pass a class a class,
//! of no one identity, so `strategy.method_defined?(:authenticate!)` and
//! `strategy.ancestors` ask whichever class it is. Spinel builds no class
//! at run time and answers no Module query on one of several classes, so
//! its emit still refuses both.

use super::emit_and_run;
use roundhouse::project::BuildTarget;

const REGISTRY: &str = r#"class Registry
  class Base
    def run
      "base"
    end
  end

  class Plain < Base
    def run
      "plain"
    end
  end

  @@kinds = {}

  def self.add(label, kind = nil)
    kind ||= Class.new(Registry::Base)
    raise "no run" unless kind.method_defined?(:run)
    raise "not a base" unless kind.ancestors.include?(Registry::Base)
    @@kinds[label] = kind
  end

  def self.run(label)
    @@kinds[label].new.run
  end
end
"#;

const SCRIPT: &str = r#"raise "got #{Registry.run(:plain)}" unless Registry.run(:plain) == "plain"
puts "class parameter passed"
"#;

fn app() -> emit_and_run::Overlay {
    emit_and_run::real_blog()
        .write("app/models/registry.rb", REGISTRY)
        .write("config/initializers/registry.rb", "Registry.add(:plain, Registry::Plain)\n")
}

#[test]
fn a_class_parameter_or_assigned_a_new_class_answers_module_queries() {
    let run = app().run_ruby(SCRIPT);
    run.assert_passes();
    assert!(run.stdout.contains("class parameter passed"), "{}", run.stdout);
}

#[test]
fn spinel_still_refuses_a_run_time_class_and_its_module_query() {
    let (_, errors) = app().emit(BuildTarget::Spinel);
    assert!(errors.iter().any(|e| e.contains("`new` requires a runtime class object")), "{errors:#?}");
    assert!(errors.iter().any(|e| e.contains("`method_defined?` on one of several classes")), "{errors:#?}");
}
