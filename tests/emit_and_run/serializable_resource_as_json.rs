//! `ActiveModelSerializers::SerializableResource.new(x, serializer: S).as_json`
//! outside a render reads what active_model_serializers 0.10.16 answers
//! there (`{id: 1, title: "Hello", shout: "HELLO", created_at: <Time>}`,
//! the attributes adapter's Symbol-keyed Hash, values as the serializer
//! reads them), with the serializer lowered and no gem.

use super::emit_and_run;

fn app() -> emit_and_run::Overlay {
    emit_and_run::real_blog()
        .write(
            "app/serializers/article_serializer.rb",
            "class ArticleSerializer < ActiveModel::Serializer\n  attributes :id, :title, :created_at\n  attribute(:shout) { object.title.upcase }\nend\n",
        )
        .write(
            "app/models/article_card.rb",
            "class ArticleCard\n  def initialize(article)\n    @article = article\n  end\n\n  def as_json\n    ActiveModelSerializers::SerializableResource.new(@article, serializer: ArticleSerializer).as_json\n  end\nend\n",
        )
}

const SCRIPT: &str = r##"a = Article.create!(title: "Hello", body: "A long body")
h = ArticleCard.new(a).as_json
want = { id: a.id, title: "Hello", created_at: a.created_at, shout: "HELLO" }
raise "got #{h.inspect}" unless h == want && h.keys == want.keys
raise "created_at is #{h[:created_at].class}" if h[:created_at].is_a?(String)
puts h.keys.inspect
"##;

#[test]
fn a_serializable_resource_as_json_reads_the_serializers_hash() {
    let run = app().run_ruby(SCRIPT);
    run.assert_passes();
    assert_eq!(run.stdout, "[:id, :title, :created_at, :shout]\n", "stderr:\n{}", run.stderr);
}

#[test]
fn a_serializable_resource_as_json_emits_for_spinel() {
    let (_emitted, errors) = app().emit(roundhouse::project::BuildTarget::Spinel);
    assert!(errors.is_empty(), "{errors:#?}");
}
