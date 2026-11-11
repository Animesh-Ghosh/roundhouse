//! `Model.reflect_on_association(name)` answers the association's macro, foreign key and `polymorphic:` as Rails'
//! AssociationReflection does (orm_adapter's `conditions_to_fields` asks `belongs_to?`, `foreign_key` and
//! `options[:polymorphic]`), and `base_class` is the class itself for a model no STI parent has.

use super::emit_and_run;

const SCRIPT: &str = r#"belongs = Comment.reflect_on_association(:article)
raise "belongs_to macro #{belongs.macro.inspect}" unless belongs.macro == :belongs_to && belongs.belongs_to?
raise "belongs_to key #{belongs.foreign_key.inspect}" unless belongs.foreign_key == "article_id"
raise "belongs_to options #{belongs.options.inspect}" unless belongs.options == {}
many = Article.reflect_on_association(:comments)
raise "has_many macro #{many.macro.inspect}" unless many.macro == :has_many && !many.belongs_to?
raise "has_many key #{many.foreign_key.inspect}" unless many.foreign_key == "article_id"
raise "none" unless Article.reflect_on_association(:nothing).nil?
raise "base_class" unless Article.base_class == Article
puts "association reflection passed"
"#;

#[test]
fn an_association_reflection_answers_its_macro_and_foreign_key() {
    let run = emit_and_run::real_blog().run_ruby(SCRIPT);
    run.assert_passes();
    assert!(run.stdout.contains("association reflection passed"), "{}", run.stdout);
}

#[test]
#[ignore = "requires the Spinel toolchain"]
fn an_association_reflection_answers_its_macro_and_foreign_key_natively() {
    let run = emit_and_run::real_blog().run_spinel(SCRIPT);
    run.assert_passes();
    assert!(run.stdout.contains("association reflection passed"), "{}", run.stdout);
}
