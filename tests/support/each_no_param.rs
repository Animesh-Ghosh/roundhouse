//! #1910: a block with no parameter, which Ruby runs once per element
//! without binding it. Each line is tagged `L<n>:`, and `RENDERS` is what
//! Rails 8.1.4 renders (the ruby target and Spinel check each line's
//! whole output, the strict targets that the page holds each one,
//! `content_tag_options::app_rendering`).

use super::emit_and_run::Overlay;

pub const VIEW: &str = concat!(
    "<% [1, 2].each do %>L0:<%= @article.id %>|<% end %>\n",
    "L1:<%= [1, 2].map { \"x\" }.join(\",\") %>\n",
);

pub const RENDERS: &[&str] = &["L0:7|L0:7|", "L1:x,x"];

/// `base` (a probe app over `fixtures/real-blog`) with the record the
/// view reads.
pub fn app(base: Overlay) -> Overlay {
    super::nested_route_helper_record::app(base, false)
}

/// The strict targets' probe app.
pub fn strict_app() -> Overlay {
    app(super::content_tag_options::app_rendering(VIEW, RENDERS))
}
