//! #1802: a view's `record.pred?` where the model defines `pred?` itself.
//! The Go model emit writes it as the method `<Name>Pred()`; the view read
//! it as the field `record.<Name>`, which the struct doesn't have.

use super::emit_and_run;

/// real-blog with `Article#featured?`, the articles index rendering it per
/// article, and an integration test asserting it renders `F:true`: the
/// fixture articles are saved, so their ids are positive.
pub fn app() -> emit_and_run::Overlay {
    emit_and_run::real_blog()
        .edit(
            "app/models/article.rb",
            "  has_many :comments, dependent: :destroy\n",
            "  has_many :comments, dependent: :destroy\n\n  def featured?\n    id > 0\n  end\n",
        )
        .edit(
            "app/views/articles/index.html.erb",
            "<% content_for :title, \"Articles\" %>\n",
            "<% content_for :title, \"Articles\" %>\n<% @articles.each do |a| %><p class=\"featured\">F:<%= a.featured? %></p><% end %>\n",
        )
        .write("test/controllers/articles_featured_controller_test.rb", TEST)
}

const TEST: &str = "require \"test_helper\"\n\nclass ArticlesFeaturedControllerTest < ActionDispatch::IntegrationTest\n  test \"the index renders each article's featured answer\" do\n    get articles_url\n    assert_response :success\n    assert_includes response.body, \"F:true\"\n  end\nend\n";

/// Go's test emit has no `assert_includes`; a probe defines it.
pub const GO_ASSERT_INCLUDES: &str = "package v2\n\nimport \"strings\"\n\nfunc assert_includes(body string, want string) {\n\tif !strings.Contains(body, want) {\n\t\tpanic(\"missing \" + want + \" in \" + body)\n\t}\n}\n";
