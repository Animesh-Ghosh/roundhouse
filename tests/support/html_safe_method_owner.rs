//! A method whose body ends in `.html_safe` is written unescaped in a
//! template only on a receiver that has that definition: its class, a
//! subclass of it, or the module itself. Another class's method of the
//! same name is escaped, as Rails 8.1.4 escapes it. Each line of `VIEW`
//! is tagged, and `RENDERS` is what Rails 8.1.4 renders for it.

use super::emit_and_run;

pub const VIEW: &str = concat!(
    "A:<%= @article.label %>\n",
    "C:<%= @comment.label %>\n",
    "F:<%= Formatting.badge %>\n",
    "P:<%= Plain.badge %>\n",
    "U:<%= @article.tag_html %>\n",
    "X:<% x = @comment.nil? ? @article : @comment %><%= x.label %>\n",
    "Z:<% z = @comment.nil? ? @article : @comment %><%= z.tag_html %>\n",
);

pub const RENDERS: &[&str] = &["A:<b>a</b>", "C:&lt;c&gt;", "F:<s>", "P:&lt;p&gt;", "U:<u>", "X:&lt;c&gt;", "Z:<u>"];

/// What a name-only match wrote: another class's `label`, another
/// module's `badge`, and `label` on an `Article | Comment` receiver
/// (only `Article` has the marked one), unescaped.
pub const ABSENT: &[&str] = &["C:<c>", "P:<p>", "X:<c>"];

/// real-blog with `GET /probe` rendering `VIEW`:
/// - `Article#label` and `ApplicationRecord#tag_html` return html-safe
///   markup; `Comment#label` returns a plain String;
/// - `Formatting.badge` is html-safe, `Plain.badge` isn't;
/// - `x` and `z` are typed `Article | Comment`: `label` is marked only
///   on `Article`, so it is escaped; `tag_html` is marked on both
///   classes' parent, so it is trusted (which shows the receiver's
///   classes are known, not untyped).
pub fn app() -> emit_and_run::Overlay {
    emit_and_run::real_blog()
        .edit(
            "app/models/application_record.rb",
            "  primary_abstract_class\n",
            "  primary_abstract_class\n\n  def tag_html\n    \"<u>\".html_safe\n  end\n",
        )
        .edit(
            "app/models/article.rb",
            "  validates :title, presence: true\n",
            "  validates :title, presence: true\n\n  def label\n    \"<b>a</b>\".html_safe\n  end\n",
        )
        .edit(
            "app/models/comment.rb",
            "  validates :commenter, presence: true\n",
            "  validates :commenter, presence: true\n\n  def label\n    \"<c>\"\n  end\n",
        )
        .write("lib/formatting.rb", "module Formatting\n  def self.badge\n    \"<s>\".html_safe\n  end\nend\n")
        .write("lib/plain.rb", "module Plain\n  def self.badge\n    \"<p>\"\n  end\nend\n")
        .write(
            "app/controllers/probes_controller.rb",
            "class ProbesController < ApplicationController\n  def show\n    @article = Article.new\n    @comment = Comment.new\n  end\nend\n",
        )
        .write("app/views/probes/show.html.erb", VIEW)
        .edit("config/routes.rb", "  root \"articles#index\"\n", "  root \"articles#index\"\n  get \"/probe\", to: \"probes#show\", as: :probe\n")
}

/// A minitest integration test asserting each of `RENDERS`, and none of
/// `ABSENT`, in the probe's body.
pub fn integration_test() -> String {
    let mut asserts = String::new();
    for r in RENDERS {
        asserts.push_str(&format!("    assert_includes response.body, {}\n", ruby_str(r)));
    }
    for a in ABSENT {
        asserts.push_str(&format!("    refute_includes response.body, {}\n", ruby_str(a)));
    }
    format!(
        "require \"test_helper\"\n\nclass ProbesControllerTest < ActionDispatch::IntegrationTest\n  test \"html safe methods by owner\" do\n    get probe_url\n    assert_response :success\n{asserts}  end\nend\n"
    )
}

/// The same checks as a script, for the Spinel binary.
pub fn render_script() -> String {
    let mut script = String::from(concat!(
        "Db.configure(\":memory:\")\n",
        "Schema.statements.each { |sql| Db.exec(sql) }\n",
        "ActiveRecord.adapter = SqliteAdapter\n",
        "controller = ProbesController.new\n",
        "controller.request_method = \"GET\"\n",
        "controller.process_action(:show)\n",
        "body = controller.body\n",
    ));
    for r in RENDERS {
        script.push_str(&format!("raise \"missing \" + {0} + \" in \" + body unless body.include?({0})\n", ruby_str(r)));
    }
    for a in ABSENT {
        script.push_str(&format!("raise \"unexpected \" + {0} + \" in \" + body if body.include?({0})\n", ruby_str(a)));
    }
    script.push_str("puts \"html safe owners rendered\"\n");
    script
}

fn ruby_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}
