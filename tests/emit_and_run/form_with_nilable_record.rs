//! A form's record loaded by a finder that can answer nil (`find_by`,
//! `first`, `last`) names its fields after its model, as Rails does, not
//! after the view's directory (#1757).

use super::emit_and_run;

const TEST: &str = r#"require "test_helper"

class NilableRecordFormControllerTest < ActionDispatch::IntegrationTest
  test "the fields are named after the record's model" do
    Comment.create!(article: articles(:one), commenter: "Ann", body: "b")
    get edit_article_url(articles(:one))
    assert_response :success
    assert_select "input#comment_commenter[type=text]"
    assert_select "input#article_commenter", count: 0
  end
end
"#;

#[test]
fn a_form_on_a_nilable_finders_record_names_its_fields_after_the_model() {
    for finder in ["Comment.find_by(commenter: \"Ann\")", "Comment.first", "Comment.last"] {
        emit_and_run::real_blog()
            .edit("app/controllers/articles_controller.rb", "  def edit\n  end\n", &format!("  def edit\n    @comment = {finder}\n  end\n"))
            .edit(
                "app/views/articles/edit.html.erb",
                "  <%= render \"form\", article: @article %>\n",
                "  <%= render \"form\", article: @article %>\n  <%= form_with(model: @comment, url: \"/c\") do |f| %><%= f.text_field :commenter %><% end %>\n",
            )
            .write("test/controllers/nilable_record_form_controller_test.rb", TEST)
            .run_test("test/controllers/nilable_record_form_controller_test.rb")
            .assert_passes();
    }
}
