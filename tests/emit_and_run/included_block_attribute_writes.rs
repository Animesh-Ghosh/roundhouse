//! A Concern's `included do` that declares `class_attribute`s and then
//! gives them values, by `self.name =` or through one of its class
//! methods that only writes them. Every includer gets the attributes
//! with those values (responders' `ActionController::RespondWith`).

use super::emit_and_run;
use emit_and_run::Overlay;
use roundhouse::project::BuildTarget;

const CONCERN: &str = r#"module Sized
  extend ActiveSupport::Concern

  included do
    class_attribute :page_size, :unit
    self.page_size = 50
    reset_unit
  end

  class_methods do
    def reset_unit
      self.unit = "items"
    end
  end
end
"#;

const OTHER_STATEMENT: &str = r#"module Sized
  extend ActiveSupport::Concern

  included do
    class_attribute :page_size
    self.page_size = 50
    Registry.note(self)
  end
end
"#;

fn app(concern: &str) -> Overlay {
    emit_and_run::empty_app()
        .write("app/models/sized.rb", concern)
        .write("app/models/registry.rb", "class Registry\n  def self.note(klass)\n    klass\n  end\nend\n")
        .write("app/models/listing.rb", "class Listing\n  include Sized\nend\n")
        .write("app/models/short_listing.rb", "class ShortListing < Listing\n  self.page_size = 5\nend\n")
}

#[test]
fn values_written_in_the_block_reach_every_includer() {
    app(CONCERN)
        .run_ruby(
            r#"
raise "page_size #{Listing.page_size.inspect}" unless Listing.page_size == 50
raise "unit #{Listing.unit.inspect}" unless Listing.unit == "items"
raise "instance #{Listing.new.page_size.inspect}" unless Listing.new.page_size == 50
raise "subclass #{ShortListing.page_size.inspect}" unless ShortListing.page_size == 5
raise "parent moved" unless Listing.page_size == 50
raise "inherited unit" unless ShortListing.unit == "items"
puts "ALL OK"
"#,
        )
        .assert_passes();
}

#[test]
fn the_block_is_expanded_for_spinel() {
    let (emitted, _errors) = app(CONCERN).emit(BuildTarget::Spinel);
    let source = std::fs::read_to_string(emitted.join("app/models/listing.rb")).expect("listing.rb emitted");
    assert!(source.contains("def self.page_size"), "no reader on the includer:\n{source}");
    assert!(source.contains("def self.unit"), "no reader on the includer:\n{source}");
}

#[test]
fn a_block_doing_anything_else_is_left_alone() {
    let (emitted, _errors) = app(OTHER_STATEMENT).emit(BuildTarget::Spinel);
    let source = std::fs::read_to_string(emitted.join("app/models/listing.rb")).expect("listing.rb emitted");
    assert!(!source.contains("def self.page_size"), "the block's other statement was dropped:\n{source}");
}

// Ruby calls the includer's own class method, so the Concern's body says nothing about what the block writes.
#[test]
fn an_includer_with_its_own_method_keeps_the_block() {
    let (emitted, _errors) = app(CONCERN)
        .write("app/models/listing.rb", "class Listing\n  def self.reset_unit\n    self.unit = \"rows\"\n  end\n\n  include Sized\nend\n")
        .emit(BuildTarget::Spinel);
    let source = std::fs::read_to_string(emitted.join("app/models/listing.rb")).expect("listing.rb emitted");
    assert!(!source.contains("def self.page_size"), "the includer's own method was bypassed:\n{source}");
}

// A seed has no order against the others, so a value may not depend on one.
#[test]
fn a_written_value_that_reads_the_class_keeps_the_block() {
    for write in ["self.page_size = self.unit", "self.page_size = unit", "self.page_size = @unit"] {
        let concern = format!(
            "module Sized\n  extend ActiveSupport::Concern\n\n  included do\n    class_attribute :page_size, :unit\n    self.unit = 7\n    {write}\n  end\nend\n"
        );
        let (emitted, _errors) = app(&concern).emit(BuildTarget::Spinel);
        let source = std::fs::read_to_string(emitted.join("app/models/listing.rb")).expect("listing.rb emitted");
        assert!(!source.contains("def self.page_size"), "{write} was seeded out of order:\n{source}");
    }
}
