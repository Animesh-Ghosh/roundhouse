//! #1862: Rails evaluates `link_to_if`'s and `link_to_unless`'s
//! condition, name and url, in that order, before the helper picks a
//! branch, and before the block runs. `Tracker.yes`/`Tracker.no` log a
//! `c`, `Tracker.label` an `n`, `Tracker.url` a `u`; `Tracker.take`
//! returns the log and clears it. Each line of `VIEW` is tagged `L<n>:`,
//! and `RENDERS` is what Rails 8.1.4 renders after the tag.

pub const VIEW: &str = concat!(
    "L0:<%= link_to_if Tracker.no, Tracker.label, Tracker.url %>|<%= Tracker.take %>\n",
    "L1:<%= link_to_unless Tracker.yes, Tracker.label, Tracker.url %>|<%= Tracker.take %>\n",
    "L2:<%= link_to_if Tracker.yes, Tracker.label, Tracker.url %>|<%= Tracker.take %>\n",
    "L3:<%= link_to_unless Tracker.no, Tracker.label, Tracker.url %>|<%= Tracker.take %>\n",
    "L4:<%= link_to_if Tracker.no, Tracker.label, Tracker.url do |name| %>[<%= name %>]<% end %>|<%= Tracker.take %>\n",
    "L5:<%= link_to_unless Tracker.yes, Tracker.label, Tracker.url do %>none<% end %>|<%= Tracker.take %>\n",
    "L6:<%= link_to_if Tracker.no, \"a\", Tracker.url %>|<%= Tracker.take %>\n",
    "L7:<%= link_to_if Tracker.no, Tracker.label, \"/x\" %>|<%= Tracker.take %>\n",
    "L8:<%= link_to_if Tracker.no, Tracker.label, Tracker.url, class: \"k\" %>|<%= Tracker.take %>\n",
    "L9:<%= link_to_if Tracker.no, Tracker.label, Tracker.url do %><%= Tracker.take %>:<% end %>|<%= Tracker.take %>\n",
    "<% x = \"o\" %>L10:<%= link_to_if Tracker.no, Tracker.label, Tracker.at(x) do |x| %>[<%= x %>]<% end %>|<%= Tracker.take %>\n",
    "<% x = \"o\" %>L11:<%= link_to_unless Tracker.yes, Tracker.label, Tracker.at(x) do |x| %>[<%= x %>]<% end %>|<%= Tracker.take %>\n",
    "<% x = \"o\" %>L12:<%= link_to_if Tracker.yes, \"i\", Tracker.at(x) do |x| %>[<%= x %>]<% end %>|<%= Tracker.take %>\n",
    "<% x = \"o\" %>L13:<%= link_to_if Tracker.no, \"i\", Tracker.at(x) do |x| %>[<%= x %>]<% end %>|<%= x %>|<%= Tracker.take %>\n",
    "<% x = \"o\" %>L14:<%= link_to_unless Tracker.yes, \"i\", \"/u\" do |x| %><% x = \"w\" %>[<%= x %>]<% end %>|<%= x %>\n",
);

/// `lib/tracker.rb`.
pub const TRACKER: &str = "module Tracker\n  LOG = []\n\n  def self.yes\n    LOG << \"c\"\n    true\n  end\n\n  def self.no\n    LOG << \"c\"\n    false\n  end\n\n  def self.label\n    LOG << \"n\"\n    \"a\"\n  end\n\n  def self.url\n    LOG << \"u\"\n    \"/x\"\n  end\n\n  def self.at(s)\n    LOG << s\n    \"/\" + s\n  end\n\n  def self.take\n    out = LOG.join\n    LOG.clear\n    out\n  end\nend\n";

pub const RENDERS: &[&str] = &[
    "L0:a|cnu",
    "L1:a|cnu",
    "L2:<a href=\"/x\">a</a>|cnu",
    "L3:<a href=\"/x\">a</a>|cnu",
    "L4:[a]|cnu",
    "L5:none|cnu",
    "L6:a|cu",
    "L7:a|cn",
    "L8:a|cnu",
    "L9:cnu:|",
    "L10:[a]|cno",
    "L11:[a]|cno",
    "L12:<a href=\"/o\">i</a>|co",
    "L13:[i]|o|co",
    "L14:[w]|o",
];

/// The strict targets' probe: a url that is a String sum, so its discard
/// is a value a compiled target must accept as a statement (Go rejects an
/// unused one); `t` is true at runtime. `STRICT_RENDERS` is Rails 8.1.4's.
pub const STRICT_VIEW: &str = concat!(
    "<% t = \"\".empty? %>L0:<%= link_to_unless t, \"a\", \"/x\" + \"y\" %>\n",
    "L1:<%= link_to_if !t, \"b\", \"/x\" + \"y\" %>\n",
    "L2:<%= link_to_unless t, \"c\", \"/\" + \"z\" do %>none<% end %>\n",
    "L3:<%= link_to_if !t, \"d\", \"/\" + \"z\" do |name| %>[<%= name %>]<% end %>\n",
    "L4:<%= link_to_if t, \"e\", \"/\" + \"w\" %>\n",
    "<% x = \"/o\" %>L5:<%= link_to_if !t, \"f\", x + \"u\" do |x| %>[<%= x %>]<% end %>\n",
    "<% y = \"/p\" %>L6:<%= link_to_if !t, \"g\", \"/\" + \"q\" do |y| %>[<%= y %>]<% end %>|<%= y %>\n",
    "<% z = \"z\" %>L7:<%= link_to_unless t, \"h\", \"/\" + \"r\" do |z| %>none<% end %>|<%= z %>\n",
);

pub const STRICT_RENDERS: &[&str] = &["L0:a\n", "L1:b\n", "L2:none\n", "L3:[d]\n", "L4:<a href=\"/w\">e</a>\n", "L5:[f]\n", "L6:[g]|/p\n", "L7:none|z"];
