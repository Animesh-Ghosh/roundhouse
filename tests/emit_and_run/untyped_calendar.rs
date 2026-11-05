use super::emit_and_run;

/// A calendar method on a receiver inference could not type (`at` is a
/// parameter no call site types) is not grounded by `lower::time_calendar`
/// and stays a dynamic call. Rails answers it from its core extensions; the
/// CRuby overlay answers it the same way, for a Time and for a Date (the
/// app names Date, so its calendar helpers ship).
fn app() -> emit_and_run::Overlay {
    emit_and_run::real_blog().edit(
        "app/models/article.rb",
        "  validates :title, presence: true\n",
        "  validates :title, presence: true

  def self.month_of(at)
    at.all_month
  end

  def self.week_start(at)
    at.beginning_of_week(:sunday)
  end

  def self.earlier(at)
    at.months_ago(2)
  end

  def self.workday?(at)
    at.on_weekday?
  end

  def self.launch_day
    Date.new(2026, 11, 7)
  end
",
    )
}

pub const ASSERTIONS: &str = r#"t = Time.new(2026, 11, 5, 10, 30)
month = Article.month_of(t)
raise "time month: #{month.inspect}" unless month.begin == Time.new(2026, 11, 1) && month.end.day == 30 && month.cover?(t)
raise "time week: #{Article.week_start(t).inspect}" unless Article.week_start(t) == Time.new(2026, 11, 1)
raise "time earlier: #{Article.earlier(t).inspect}" unless Article.earlier(t) == Time.new(2026, 9, 5, 10, 30)
raise "time workday" unless Article.workday?(t)
d = Article.launch_day
raise "date month: #{Article.month_of(d).inspect}" unless Article.month_of(d) == (Date.new(2026, 11, 1)..Date.new(2026, 11, 30))
raise "date week: #{Article.week_start(d).inspect}" unless Article.week_start(d) == Date.new(2026, 11, 1)
raise "date earlier: #{Article.earlier(d).inspect}" unless Article.earlier(d) == Date.new(2026, 9, 7)
raise "date workday" if Article.workday?(d)
puts "untyped calendar passed"
"#;

#[test]
fn an_untyped_receiver_answers_calendar_methods_like_rails() {
    let (emitted, errors) = app().emit(roundhouse::project::BuildTarget::Ruby);
    assert!(errors.is_empty(), "{errors:#?}");
    let model = std::fs::read_to_string(emitted.join("app/models/article.rb")).expect("article.rb");
    assert!(model.contains("at.all_month"), "{model}");
    let run = app().run_ruby(ASSERTIONS);
    run.assert_passes();
    assert!(run.stdout.contains("untyped calendar passed"));
}
