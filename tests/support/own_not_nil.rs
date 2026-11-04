//! A class that defines its own `not_nil!`: the call is that method, on
//! every tree, not type_toolkit's, which only the CRuby tree carries.

use super::emit_and_run::{self, Overlay};

pub const LOOKUP: &str = r#"class Lookup
  def initialize(value)
    @value = value
  end

  def not_nil!
    @value || "none"
  end

  def self.read(value)
    Lookup.new(value).not_nil!
  end
end
"#;

/// What `[Lookup.read("ada"), Lookup.read(nil)]` answers under CRuby, as JSON.
#[allow(dead_code)]
pub const SUMMARY: &str = r#"["ada","none"]"#;

pub fn overlay() -> Overlay {
    emit_and_run::real_blog().write("app/models/lookup.rb", LOOKUP)
}
