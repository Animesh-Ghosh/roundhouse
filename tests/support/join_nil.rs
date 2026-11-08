//! #1949: `join` turns a nil element into an empty string. Each line is
//! tagged `L<n>:`, and `RENDERS` is what Rails 8.1.4 renders. Go builds
//! only the first line: its array literal can't hold nil beside Strings.

use super::emit_and_run::Overlay;

pub const VIEW: &str = concat!(
    "L0:<%= [1, nil, 2].join(\",\") %>|\n",
    "L1:<%= [nil, \"a\"].join(\"-\") %>|\n",
    "L2:<%= [@article.title, \"a\"].join(\"-\") %>|\n",
);

pub const RENDERS: &[&str] = &["L0:1,,2|", "L1:-a|", "L2:-a|"];

pub const GO_VIEW: &str = "L0:<%= [1, nil, 2].join(\",\") %>|\n";

pub const GO_RENDERS: &[&str] = &["L0:1,,2|"];

/// `base` with an article whose title is nil: never assigned.
pub fn app(base: Overlay) -> Overlay {
    base.write(
        "app/controllers/probes_controller.rb",
        "class ProbesController < ApplicationController\n  def show\n    @article = Article.new(id: 7, body: \"a body long enough\")\n  end\nend\n",
    )
}

/// The strict targets' probe app.
pub fn strict_app() -> Overlay {
    app(super::content_tag_options::app_rendering(VIEW, RENDERS))
}

/// Go's probe app.
pub fn strict_go_app() -> Overlay {
    app(super::content_tag_options::app_rendering(GO_VIEW, GO_RENDERS))
}
