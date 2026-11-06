//! A namespaced model's superclass must not resolve to the model itself.
//!
//! GitLab's `Ci::ApplicationRecord < ::ApplicationRecord` lost its leading
//! `::`, found itself in `module Ci`, and became its own parent. The first
//! `delegate` on a model then walked that chain until the stack overflowed,
//! which aborted `check` before it reported anything.

use std::collections::HashMap;
use std::path::PathBuf;

use roundhouse::ingest::ingest_app_from_tree;
use roundhouse::App;

const APPLICATION_RECORD: &str = "class ApplicationRecord < ActiveRecord::Base
  primary_abstract_class
end
";

fn ingest(files: &[(&str, &str)]) -> App {
    let tree: HashMap<PathBuf, Vec<u8>> = files
        .iter()
        .map(|(p, c)| (PathBuf::from(p), c.as_bytes().to_vec()))
        .collect();
    ingest_app_from_tree(tree).expect("ingest tree")
}

fn parent_of(app: &App, name: &str) -> String {
    let model = app
        .models
        .iter()
        .find(|m| m.name.0.as_str() == name)
        .unwrap_or_else(|| panic!("{name} should ingest as a model"));
    model.parent.as_ref().expect("a parent").0.as_str().to_owned()
}

fn namespaced_base(superclass: &str) -> String {
    format!(
        "module Ci
  class ApplicationRecord < {superclass}
    self.abstract_class = true
  end
end
"
    )
}

/// The shape GitLab crashed on: a sibling model delegating, beside a
/// namespaced base with the same name as the top-level one.
const DELEGATING_MODEL: &str = "module Ci
  module Catalog
    class Resource < ::ApplicationRecord
      delegate :full_path, to: :project
    end
  end
end
";

const SIBLING_MODEL: &str = "module Ci
  module Catalog
    class Component < ::ApplicationRecord
    end
  end
end
";

#[test]
fn an_absolute_superclass_skips_the_enclosing_modules() {
    let base = namespaced_base("::ApplicationRecord");
    let app = ingest(&[
        ("app/models/application_record.rb", APPLICATION_RECORD),
        ("app/models/ci/application_record.rb", &base),
        ("app/models/ci/catalog/resource.rb", DELEGATING_MODEL),
        ("app/models/ci/catalog/component.rb", SIBLING_MODEL),
    ]);
    assert_eq!(parent_of(&app, "Ci::ApplicationRecord"), "ApplicationRecord");
    assert_eq!(parent_of(&app, "Ci::Catalog::Resource"), "ApplicationRecord");
    assert_eq!(parent_of(&app, "Ci::Catalog::Component"), "ApplicationRecord");
}

#[test]
fn a_bare_superclass_does_not_find_the_class_being_declared() {
    let base = namespaced_base("ApplicationRecord");
    let app = ingest(&[
        ("app/models/application_record.rb", APPLICATION_RECORD),
        ("app/models/ci/application_record.rb", &base),
    ]);
    assert_eq!(parent_of(&app, "Ci::ApplicationRecord"), "ApplicationRecord");
}

#[test]
fn a_bare_superclass_still_prefers_the_closer_base() {
    let base = namespaced_base("::ApplicationRecord");
    let app = ingest(&[
        ("app/models/application_record.rb", APPLICATION_RECORD),
        ("app/models/ci/application_record.rb", &base),
        (
            "app/models/ci/build.rb",
            "module Ci
  class Build < ApplicationRecord
  end
end
",
        ),
    ]);
    assert_eq!(parent_of(&app, "Ci::Build"), "Ci::ApplicationRecord");
}

#[test]
fn a_parent_cycle_does_not_overflow_the_delegate_walk() {
    let app = ingest(&[
        ("app/models/application_record.rb", APPLICATION_RECORD),
        (
            "app/models/a.rb",
            "class A < B
  self.abstract_class = true
  delegate :full_path, to: :project
end
",
        ),
        (
            "app/models/b.rb",
            "class B < A
  self.abstract_class = true
  delegate :other_path, to: :project
end
",
        ),
    ]);
    // Reaching here is the assertion: the walk used to recurse forever.
    let _ = app;
}
