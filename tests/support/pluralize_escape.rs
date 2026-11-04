//! `pluralize` in a template: Rails 8.1.4 escapes its String, which isn't
//! html-safe, where the emitted views appended it as it was.

use super::emit_and_run;

pub const VIEW: &str = concat!(
    "<% w = \"<b>x</b>\" %><%= pluralize(1, w) %>\n",
    "<%= pluralize(2, \"<i>\") %>\n",
    "<%= pluralize(2, \"comment\") %>\n",
);

/// What Rails 8.1.4 renders for the lines of `VIEW`.
pub const RENDERS: &[&str] = &[
    "1 &lt;b&gt;x&lt;/b&gt;",
    "2 &lt;i&gt;s",
    "2 comments",
];

/// Neither the raw words nor a second escape of them.
pub const ABSENT: &[&str] = &["<b>x</b>", "<i>s", "&amp;lt;"];

/// real-blog with `GET /probe` rendering `VIEW`.
pub fn app() -> emit_and_run::Overlay {
    emit_and_run::real_blog()
        .write("app/controllers/probes_controller.rb", "class ProbesController < ApplicationController\n  def show\n  end\nend\n")
        .write("app/views/probes/show.html.erb", VIEW)
        .edit("config/routes.rb", "  root \"articles#index\"\n", "  root \"articles#index\"\n  get \"/probe\", to: \"probes#show\", as: :probe\n")
}

/// A minitest integration test asserting every `RENDERS` line and none
/// of `ABSENT` in `GET /probe`'s body.
pub fn integration_test() -> String {
    let mut asserts = String::new();
    for r in RENDERS {
        asserts.push_str(&format!("    assert_includes response.body, {}\n", ruby_str(r)));
    }
    for a in ABSENT {
        asserts.push_str(&format!("    refute_includes response.body, {}\n", ruby_str(a)));
    }
    format!(
        "require \"test_helper\"\n\nclass ProbesControllerTest < ActionDispatch::IntegrationTest\n  test \"pluralize is escaped\" do\n    get probe_url\n    assert_response :success\n{asserts}  end\nend\n"
    )
}

/// Ruby statements that raise unless `body` holds every `RENDERS` line and
/// none of `ABSENT`.
pub fn body_checks() -> String {
    let mut checks = String::new();
    for r in RENDERS {
        checks.push_str(&format!("raise body unless body.include?({})\n", ruby_str(r)));
    }
    for a in ABSENT {
        checks.push_str(&format!("raise body if body.include?({})\n", ruby_str(a)));
    }
    checks
}

fn ruby_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}
