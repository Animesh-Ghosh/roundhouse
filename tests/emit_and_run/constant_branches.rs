//! Branches Ruby never takes.
//!
//! - A predicate whose body is a literal, and whose name nothing else defines, answers that constant, so `if
//!   x.beta?` takes one branch and the other, typed, would report what never runs (Devise's
//!   `if devise_mapping.validatable?` over a model that is not `:validatable`).
//! - A gem method no reached call passes a block to never takes its `x if block_given?` (Warden's `Strategies.add`,
//!   called with the strategy class alone once the `update` nothing calls is left out).

use super::emit_and_run;

const FLAGS: &str = r#"class Flags
  def beta?
    false
  end
end
"#;

const MODEL: &str = r#"class Article < ApplicationRecord
  def self.release(flags)
    if flags.beta?
      flags.preview_build
    else
      "stable"
    end
  end

  def self.release_of_default
    release(Flags.new)
  end

"#;

const SCRIPT: &str = r#"raise "release #{Article.release_of_default.inspect}" unless Article.release_of_default == "stable"
puts "constant branches passed"
"#;

fn app() -> emit_and_run::Overlay {
    emit_and_run::real_blog().write("app/models/flags.rb", FLAGS).edit("app/models/article.rb", "class Article < ApplicationRecord\n", MODEL)
}

#[test]
fn a_constant_predicate_takes_one_branch() {
    let run = app().run_ruby(SCRIPT);
    run.assert_passes();
    assert!(run.stdout.contains("constant branches passed"), "{}", run.stdout);
}

#[test]
#[ignore = "requires the Spinel toolchain"]
fn a_constant_predicate_takes_one_branch_natively() {
    let run = app().run_spinel(SCRIPT);
    run.assert_passes();
    assert!(run.stdout.contains("constant branches passed"), "{}", run.stdout);
}

const REGISTRY: &str = r#"module Kinds
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

  class Registry
    @@kinds = {}

    def self.add(label, kind, &block)
      kind.class_eval(&block) if block_given?
      @@kinds[label] = kind
    end

    def self.update(label, &block)
      add(label, @@kinds[label], &block)
    end

    def self.run(label)
      @@kinds[label].new.run
    end
  end
end
"#;

fn installed() -> emit_and_run::Overlay {
    let gem = "vendor/bundle/ruby/3.4.0/gems/kinds-1.0";
    emit_and_run::real_blog()
        .edit("Gemfile.lock", "    debug (1.11.1)\n", "    debug (1.11.1)\n    kinds (1.0)\n")
        .edit("Gemfile.lock", "DEPENDENCIES\n", "DEPENDENCIES\n  kinds\n")
        .write(".bundle/config", "---\nBUNDLE_PATH: \"vendor/bundle\"\n")
        .write(&format!("{gem}/lib/kinds.rb"), "require \"kinds/registry\"\n")
        .write(&format!("{gem}/lib/kinds/registry.rb"), REGISTRY)
        .write("config/initializers/kinds.rb", "Kinds::Registry.add(:plain, Kinds::Plain)\n")
        .edit("app/models/article.rb", "class Article < ApplicationRecord\n", "class Article < ApplicationRecord\n  def self.kind_run\n    Kinds::Registry.run(:plain)\n  end\n\n")
}

#[test]
#[ignore = "requires the Spinel toolchain"]
fn a_gem_method_no_call_passes_a_block_leaves_its_block_given_branch_out() {
    let run = installed().run_spinel("raise \"run\" unless Article.kind_run == \"plain\"\nputs \"block_given passed\"\n");
    run.assert_passes();
    assert!(run.stdout.contains("block_given passed"), "{}", run.stdout);
}
