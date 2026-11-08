//! #1937: a view reads a nullable string column (`articles.title`)
//! with `==`, `!=`, `empty?` and `length`. The column is `*string` on
//! Go, `String | Nil` on Crystal and `Option<String>` on Rust, and each
//! call read it as a string. Each line is tagged `L<n>:`, and `RENDERS`
//! is what Rails 8.1.4 renders with the title present; `NIL_RENDERS`
//! is what it renders for the comparisons with the title nil, where
//! `==` is false and `!=` true (`empty?` and `length` raise on nil).

use super::emit_and_run::Overlay;

pub const VIEW: &str = concat!(
    "L0:<% if @article.title == \"t\" %>y<% end %>|\n",
    "L1:<% if @article.title.empty? %>y<% else %>n<% end %>|\n",
    "L2:<%= @article.title.length %>|\n",
    "L3:<% if @article.title != \"u\" %>y<% end %>|\n",
);

pub const RENDERS: &[&str] = &["L0:y|", "L1:n|", "L2:1|", "L3:y|"];

pub const NIL_VIEW: &str = concat!(
    "L0:<% if @article.title == \"t\" %>y<% end %>|\n",
    "L1:<% if @article.title != \"u\" %>y<% end %>|\n",
);

pub const NIL_RENDERS: &[&str] = &["L0:|", "L1:y|"];

/// `base` (a probe app over `fixtures/real-blog`) with an article whose
/// title is "t".
pub fn app(base: Overlay) -> Overlay {
    super::nested_route_helper_record::app(base, false)
}

/// `base` with an article whose title is nil: never assigned.
pub fn nil_app(base: Overlay) -> Overlay {
    base.write(
        "app/controllers/probes_controller.rb",
        "class ProbesController < ApplicationController\n  def show\n    @article = Article.new(id: 7, body: \"a body long enough\")\n  end\nend\n",
    )
}

/// The strict targets' probe apps.
pub fn strict_app() -> Overlay {
    app(super::content_tag_options::app_rendering(VIEW, RENDERS))
}

pub fn strict_nil_app() -> Overlay {
    nil_app(super::content_tag_options::app_rendering(NIL_VIEW, NIL_RENDERS))
}
