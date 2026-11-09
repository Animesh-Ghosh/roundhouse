# frozen_string_literal: true
# Inflector reopen — ruby-family surface only (connection.rb pattern):
# shipped to the scaffold trees via the project.rs stems list and
# required from inflector.rb, but NOT in the runtime_loader tables, so
# the strict-target transpilers never see the gsub-with-Regexp bodies.
# Promote to inflector.rb only with per-target regex-replace support.
module Inflector
  PARAMETERIZE_SQUASH = /[^a-z0-9\-_]+/.freeze
  PARAMETERIZE_RUNS = /-{2,}/.freeze
  PARAMETERIZE_EDGES = /\A-|-\z/.freeze

  # AS `String#parameterize`, default separator: downcase, squash
  # non-alphanumerics to "-", collapse runs, trim edges. (No
  # transliteration pass — non-ASCII chars drop like Rails'
  # post-transliterate "?" placeholders do under gsub.) Separator-kwarg
  # call sites stay on the CRuby overlay's String reopen; only the
  # zero-arg form grounds here.
  def self.parameterize(str)
    str.downcase
       .gsub(PARAMETERIZE_SQUASH, "-")
       .gsub(PARAMETERIZE_RUNS, "-")
       .gsub(PARAMETERIZE_EDGES, "")
  end

  # AS `String#pluralize(count)`: count-aware inflection of the string
  # itself, singular when `count == 1`, else the plural (NOT the
  # count-labeling `pluralize(count, word)` in inflector.rb). Rails'
  # default rules (activesupport 8.1.4), regex-free so every target can
  # carry it; the emit grounds `value.pluralize` here because the built-in
  # String can't be reopened on Spinel. A nil receiver raises, as Rails'
  # String method does.
  def self.pluralize_word(word, count)
    raise NoMethodError, "undefined method 'pluralize' for nil" if word.nil?
    s = word.to_s
    return s if count == 1 || s.empty?
    d = s.downcase
    return s if pluralize_uncountable?(d)
    irregular = pluralize_irregular(s, d)
    return irregular unless irregular.nil?
    return s + "zes" if d.end_with?("quiz")
    return s if d == "oxen"
    return s + "en" if d == "ox"
    return s if d == "mice" || d == "lice"
    return pluralize_tail(s, 4) + "ice" if d == "mouse" || d == "louse"
    return pluralize_tail(s, 2) + "ices" if pluralize_ix?(d)
    return s + "es" if d.end_with?("x") || d.end_with?("ch") || d.end_with?("ss") || d.end_with?("sh")
    return pluralize_tail(s, 1) + "ies" if pluralize_consonant_y?(d)
    return s + "s" if d.end_with?("hive")
    return pluralize_tail(s, 2) + "ves" if pluralize_fe?(d)
    return pluralize_tail(s, 1) + "ves" if d.end_with?("lf") || d.end_with?("rf")
    return pluralize_tail(s, 3) + "ses" if d.end_with?("sis")
    return s if d.end_with?("ta") || d.end_with?("ia")
    return pluralize_tail(s, 2) + "a" if d.end_with?("tum") || d.end_with?("ium")
    return s + "es" if d.end_with?("buffalo") || d.end_with?("tomato")
    return s + "es" if d.end_with?("bus")
    return s + "es" if d.end_with?("alias") || d.end_with?("status")
    return s if d.end_with?("octopi") || d.end_with?("viri")
    return pluralize_tail(s, 2) + "i" if d.end_with?("octopus") || d.end_with?("virus")
    return pluralize_tail(s, 2) + "es" if d == "axis" || d == "testis"
    return pluralize_tail(s, 1) + "s" if d.end_with?("s")
    s + "s"
  end

  def self.pluralize_tail(s, drop)
    s[0, s.length - drop].to_s
  end

  def self.pluralize_uncountable?(d)
    d == "equipment" || d == "information" || d == "rice" || d == "money" || d == "species" ||
      d == "series" || d == "fish" || d == "sheep" || d == "jeans" || d == "police"
  end

  # Rails' `inflect.irregular` rules match the tail, and keep the first
  # letter's case. The answer is nil when none applies.
  def self.pluralize_irregular(s, d)
    return pluralize_tail(s, 6) + s[s.length - 6, 1].to_s + "eople" if d.end_with?("person")
    return s if d.end_with?("people") || d.end_with?("children")
    return pluralize_tail(s, 2) + "en" if d.end_with?("man")
    return s if d.end_with?("men") || d.end_with?("sexes") || d.end_with?("moves") || d.end_with?("zombies")
    return s + "ren" if d.end_with?("child")
    return s + "es" if d.end_with?("sex")
    return s + "s" if d.end_with?("move") || d.end_with?("zombie")
    nil
  end

  def self.pluralize_ix?(d)
    d.end_with?("matrix") || d.end_with?("matrex") || d.end_with?("vertix") || d.end_with?("vertex") ||
      d.end_with?("indix") || d.end_with?("index")
  end

  def self.pluralize_consonant_y?(d)
    return false unless d.end_with?("y") && d.length > 1
    return true if d.end_with?("quy")
    prev = d[d.length - 2, 1].to_s
    !(prev == "a" || prev == "e" || prev == "i" || prev == "o" || prev == "u" || prev == "y")
  end

  def self.pluralize_fe?(d)
    return false unless d.end_with?("fe") && d.length > 2
    d[d.length - 3, 1].to_s != "f"
  end
end
