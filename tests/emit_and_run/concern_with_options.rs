//! #1722: a concern's `included do` block with `with_options class_name:
//! … do has_many … end` lost the options: the association's class fell
//! back to its name (`Note`), a class nothing defines. Written in the
//! model itself, the block applied them.

use super::emit_and_run;

const CONCERN: &str = "module Notable\n  extend ActiveSupport::Concern\n\n  included do\n    with_options class_name: \"Comment\", dependent: :destroy do\n      has_many :notes, foreign_key: \"article_id\"\n    end\n  end\nend\n";

fn app() -> emit_and_run::Overlay {
    emit_and_run::real_blog()
        .write("app/models/concerns/notable.rb", CONCERN)
        .edit("app/models/article.rb", "  has_many :comments, dependent: :destroy\n", "  has_many :comments, dependent: :destroy\n  include Notable\n")
}

#[test]
fn a_concern_with_options_block_applies_its_options() {
    let (emitted, errors) = app().emit(roundhouse::project::BuildTarget::Ruby);
    assert!(errors.is_empty(), "{errors:?}");
    let article = std::fs::read_to_string(emitted.join("app/models/article.rb")).unwrap();
    let notes = article.split("def notes\n").nth(1).expect("def notes").split("\n  end\n").next().unwrap();
    assert!(notes.contains("FROM comments") && !notes.contains("Note."), "{notes}");
}

/// The association answers its rows: the article's comments.
#[test]
fn a_concern_with_options_association_reads_its_class() {
    let test = "require \"test_helper\"\n\nclass NotesTest < ActiveSupport::TestCase\n  test \"notes are the article's comments\" do\n    article = articles(:one)\n    assert_equal article.comments.map(&:id).sort, article.notes.map(&:id).sort\n    assert_operator article.notes.size, :>, 0\n  end\nend\n";
    app().write("test/models/notes_test.rb", test).run_test("test/models/notes_test.rb").assert_passes();
}
