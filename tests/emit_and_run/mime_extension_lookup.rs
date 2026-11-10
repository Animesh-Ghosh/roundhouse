//! Devise's `navigational_formats` selects the formats `Mime::EXTENSION_LOOKUP[format.to_s]` registers:
//! the registered type for an extension, or nil, as `Mime::Type.lookup_by_extension` answers it.

use super::emit_and_run;

const MODEL: &str = r#"
  def self.registered_formats(formats)
    formats.select { |format| Mime::EXTENSION_LOOKUP[format.to_s] }
  end

  def self.html_type
    Mime::EXTENSION_LOOKUP["html"].to_s
  end
"#;

const SCRIPT: &str = r#"got = Article.registered_formats(["html", "nope", "json"])
raise got.inspect unless got == ["html", "json"]
raise Article.html_type unless Article.html_type == "text/html"
puts "mime lookup passed"
"#;

fn app() -> emit_and_run::Overlay {
    emit_and_run::real_blog().edit("app/models/article.rb", "class Article < ApplicationRecord\n", &format!("class Article < ApplicationRecord\n{MODEL}"))
}

#[test]
fn mime_extension_lookup_answers_the_registered_type() {
    let run = app().run_ruby(SCRIPT);
    run.assert_passes();
    assert!(run.stdout.contains("mime lookup passed"), "{}", run.stdout);
}

#[test]
#[ignore = "requires the Spinel toolchain"]
fn mime_extension_lookup_answers_the_registered_type_natively() {
    let run = app().run_spinel(SCRIPT);
    run.assert_passes();
    assert!(run.stdout.contains("mime lookup passed"), "{}", run.stdout);
}
