//! The go target's `test_compat_test.go` shims each route helper under its
//! legacy name (`ArticlePath(id)` → `RouteHelpers_article_path(id)`). A
//! route Rails names no helper for still carries an action-name fallback
//! in `as_name`; for an unmodeled endpoint that is `lambda endpoint`, and
//! its shim was `func Lambda endpointPath(id int64)`, which does not
//! compile, for a `RouteHelpers_…_path` that is never generated. Only
//! named routes get a shim.

use std::collections::HashMap;
use std::path::PathBuf;

const ROUTES: &str = "Rails.application.routes.draw do\n  resources :articles, only: [:show]\n  get \"/hooks/:id\", to: ->(env) { [200, {}, [\"ok\"]] }\nend\n";

fn files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("app/models/application_record.rb", "class ApplicationRecord < ActiveRecord::Base\n  primary_abstract_class\nend\n"),
        ("app/models/article.rb", "class Article < ApplicationRecord\nend\n"),
        ("db/schema.rb", "ActiveRecord::Schema.define do\n  create_table \"articles\", force: :cascade do |t|\n    t.string \"title\"\n  end\nend\n"),
        ("config/routes.rb", ROUTES),
        ("test/test_helper.rb", "require \"rails/test_help\"\n"),
        ("test/models/article_test.rb", "require \"test_helper\"\n\nclass ArticleTest < ActiveSupport::TestCase\n  test \"counts\" do\n    assert_equal 0, Article.count\n  end\nend\n"),
    ]
}

#[test]
fn only_named_routes_get_a_test_compat_shim() {
    let tree: HashMap<PathBuf, Vec<u8>> =
        files().into_iter().map(|(p, c)| (PathBuf::from(p), c.as_bytes().to_vec())).collect();
    let mut app = roundhouse::ingest::ingest_app_from_tree(tree).expect("ingest");
    roundhouse::session::analyze_and_lower(&mut app);
    let compat = roundhouse::emit::go::emit(&app)
        .into_iter()
        .find(|f| f.path.to_string_lossy().ends_with("test_compat_test.go"))
        .map(|f| f.content)
        .expect("test_compat_test.go");
    assert!(
        compat.contains("func ArticlePath(id int64) string { return RouteHelpers_article_path(id) }"),
        "{compat}"
    );
    assert!(!compat.contains("endpoint"), "{compat}");
}
