//! Warden's `Strategies.add(label, strategy = nil)`: every caller passes
//! the class, so the parameter is never its `nil` default and `strategy
//! ||= Class.new(Base)` never assigns (`lower::bool_fold`); the parameter
//! stays a class, of no one identity, so `strategy.method_defined?
//! (:authenticate!)` and `strategy.ancestors` ask whichever class it is.
//! Only a gem's method: an app method's tests may leave the argument out.
//! Two callers pass two classes, so the parameter is one of several classes,
//! which Spinel answers `method_defined?` and `include?` on.

use super::emit_and_run;

const REGISTRY: &str = r#"class Registry
  module Runnable
  end

  class Base
    include Runnable

    def run
      "base"
    end
  end

  class Plain < Base
    def run
      "plain"
    end
  end

  class Fancy < Base
    def run
      "fancy"
    end
  end

  @@kinds = {}

  def self.add(label, kind = nil)
    kind ||= Class.new(Registry::Base)
    raise "no run" unless kind.method_defined?(:run)
    raise "not a base" unless kind.ancestors.include?(Registry::Base)
    raise "not runnable" unless kind.include?(Registry::Runnable)
    @@kinds[label] = kind
  end

  def self.run(label)
    @@kinds[label].new.run
  end
end
"#;

const SCRIPT: &str = r#"got = [Registry.run(:plain), Registry.run(:fancy)]
raise "got #{got.inspect}" unless got == ["plain", "fancy"]
puts "class parameter passed"
"#;

fn app() -> emit_and_run::Overlay {
    let gem = "vendor/bundle/ruby/3.4.0/gems/kinds-1.0";
    emit_and_run::real_blog()
        .edit("Gemfile.lock", "    debug (1.11.1)\n", "    debug (1.11.1)\n    kinds (1.0)\n")
        .edit("Gemfile.lock", "DEPENDENCIES\n", "DEPENDENCIES\n  kinds\n")
        .write(".bundle/config", "---\nBUNDLE_PATH: \"vendor/bundle\"\n")
        .write("roundhouse.yml", "extensions:\n  - config/roundhouse\n")
        .write("config/roundhouse/gems.yml", "source_gems:\n  - {gem: \"kinds\"}\n")
        .write(&format!("{gem}/lib/kinds.rb"), "require \"kinds/registry\"\n")
        .write(&format!("{gem}/lib/kinds/registry.rb"), REGISTRY)
        .write("config/initializers/registry.rb", "Registry.add(:plain, Registry::Plain)\nRegistry.add(:fancy, Registry::Fancy)\n")
}

#[test]
fn a_class_parameter_or_assigned_a_new_class_answers_module_queries() {
    let run = app().run_ruby(SCRIPT);
    run.assert_passes();
    assert!(run.stdout.contains("class parameter passed"), "{}", run.stdout);
}

#[test]
fn spinel_answers_module_queries_on_either_registered_class() {
    let (_, errors) = app().emit(roundhouse::project::BuildTarget::Spinel);
    assert!(errors.iter().all(|e| !e.contains("Module protocol")), "{errors:#?}");
}

#[test]
#[ignore = "requires the Spinel toolchain"]
fn a_class_parameter_or_assigned_a_new_class_answers_module_queries_natively() {
    let run = app().run_spinel(SCRIPT);
    run.assert_passes();
    assert!(run.stdout.contains("class parameter passed"), "{}", run.stdout);
}
