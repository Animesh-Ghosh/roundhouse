//! A `define_method` loop over a constant the class body changes in place
//! is not unrolled from the literal it was first assigned: the loop would
//! run over what the constant holds then. Spinel reports it as the
//! runtime-defined methods it is.

use super::emit_and_run;
use roundhouse::project::BuildTarget;

#[test]
fn a_loop_over_a_constant_appended_to_is_not_unrolled_from_its_first_value() {
    let (_emitted, errors) = emit_and_run::real_blog()
        .edit(
            "app/models/article.rb",
            "class Article < ApplicationRecord\n",
            "class Article < ApplicationRecord\n  KINDS = %w[draft]\n  KINDS << \"live\"\n\n  KINDS.each do |kind|\n    define_method(\"#{kind}_label\") { kind.upcase }\n  end\n\n",
        )
        .emit(BuildTarget::Spinel);
    assert!(errors.iter().any(|e| e.contains("runtime-defined method")), "{errors:#?}");
}
