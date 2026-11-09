//! A gem compiled from source that writes `begin require "x" … rescue LoadError … end` at its top
//! level loads the `begin` side when the tree provides `x`, as Ruby does; devise's
//! `rails/deprecated_constant_accessor.rb` so names `ActiveSupport::Deprecation::DeprecatedConstantAccessor`,
//! and a module that includes the constant holding it includes that module.

use roundhouse::build_target::BuildTarget;

use super::emit_and_run;

const SHELF: &str = r#"begin
  require "active_support/deprecation/constant_accessor"

  module Shelf
    Accessor = ActiveSupport::Deprecation::DeprecatedConstantAccessor
    LOADED = "rails"
  end
rescue LoadError
  module Shelf
    module Accessor
    end
    LOADED = "fallback"
  end
end

module Shelf
  class Box
    include Shelf::Accessor

    def self.loaded
      Shelf::LOADED
    end
  end
end
"#;

fn app() -> emit_and_run::Overlay {
    let fixture = roundhouse::fixtures::real_blog();
    let lock = std::fs::read_to_string(fixture.join("Gemfile.lock")).unwrap();
    let lock = lock.replacen("\n    rack (", "\n    shelf (1.0.0)\n    rack (", 1).replacen("DEPENDENCIES\n", "DEPENDENCIES\n  shelf\n", 1);
    let gemfile = std::fs::read_to_string(fixture.join("Gemfile")).unwrap() + "gem \"shelf\"\n";
    emit_and_run::real_blog()
        .write("Gemfile", &gemfile)
        .write("Gemfile.lock", &lock)
        .write(".bundle/config", "---\nBUNDLE_PATH: \"vendor/bundle\"\n")
        .write("vendor/bundle/ruby/3.4.0/gems/shelf-1.0.0/lib/shelf.rb", SHELF)
        .edit("app/models/article.rb", "class Article < ApplicationRecord\n", "class Article < ApplicationRecord\n  def self.shelf_loaded\n    Shelf::Box.loaded\n  end\n\n")
}

#[test]
fn a_gems_load_error_fallback_is_left_where_the_tree_provides_the_feature() {
    let (tree, errors) = app().emit(BuildTarget::Spinel);
    assert!(!errors.iter().any(|e| e.contains("shelf")), "{errors:#?}");
    let shelf = std::fs::read_to_string(tree.join("app/models/shelf.rb")).expect("shelf.rb");
    assert!(shelf.contains("Accessor = ActiveSupport::Deprecation::DeprecatedConstantAccessor"), "{shelf}");
    assert!(!shelf.contains("fallback"), "{shelf}");
}

#[test]
#[ignore = "requires the Spinel toolchain"]
fn a_gems_load_error_fallback_is_left_natively() {
    let run = app().run_spinel("raise Article.shelf_loaded unless Article.shelf_loaded == \"rails\"\nputs \"loaded begin passed\"\n");
    run.assert_passes();
    assert!(run.stdout.contains("loaded begin passed"), "{}", run.stdout);
}
