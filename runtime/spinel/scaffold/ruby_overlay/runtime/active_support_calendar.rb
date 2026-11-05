# frozen_string_literal: true
# Not left to lower::time_calendar alone: a receiver inference could not type keeps its dynamic call, which Rails answers here.
module ActiveSupport
  module CalendarDispatch
    WEEK_DAYS = %i[sunday monday tuesday wednesday thursday friday saturday].freeze

    def self.wday(start)
      return 1 if start.nil?
      WEEK_DAYS.index(start) || raise(ArgumentError, "Invalid beginning of week: #{start}")
    end

    def self.span(day)
      (wday(day) - 1) % 7
    end

    def self.keep_time(from, result, same_time)
      return result unless same_time
      ActiveSupport.days_since(from, ActiveSupport.civil_days(result.year, result.month, result.day) - ActiveSupport.civil_days(from.year, from.month, from.day))
    end

    TIME = {
      beginning_of_minute: :beginning_of_minute, at_beginning_of_minute: :beginning_of_minute,
      end_of_minute: :end_of_minute, at_end_of_minute: :end_of_minute,
      beginning_of_hour: :beginning_of_hour, at_beginning_of_hour: :beginning_of_hour,
      end_of_hour: :end_of_hour, at_end_of_hour: :end_of_hour,
      beginning_of_day: :beginning_of_day, at_beginning_of_day: :beginning_of_day,
      midnight: :beginning_of_day, at_midnight: :beginning_of_day,
      end_of_day: :end_of_day, at_end_of_day: :end_of_day,
      noon: :noon, at_noon: :noon, middle_of_day: :noon, at_middle_of_day: :noon,
      beginning_of_month: :beginning_of_month, at_beginning_of_month: :beginning_of_month,
      end_of_month: :end_of_month, at_end_of_month: :end_of_month,
      beginning_of_year: :beginning_of_year, at_beginning_of_year: :beginning_of_year,
      end_of_year: :end_of_year, at_end_of_year: :end_of_year,
      yesterday: :yesterday, tomorrow: :tomorrow,
      on_weekend?: :on_weekend?, on_weekday?: :on_weekday?
    }.freeze
    TIME_COUNTED = {
      days_since: :days_since, next_day: :days_since, days_ago: :days_ago, prev_day: :days_ago,
      weeks_since: :weeks_since, weeks_ago: :weeks_ago,
      months_since: :months_since, next_month: :months_since, months_ago: :months_ago, prev_month: :months_ago,
      last_month: :months_ago,
      years_since: :years_since, next_year: :years_since, years_ago: :years_ago, prev_year: :years_ago,
      last_year: :years_ago
    }.freeze
    TIME_NOW = %i[today? yesterday? tomorrow? past? future?].freeze

    DATE = {
      beginning_of_month: :date_beginning_of_month, at_beginning_of_month: :date_beginning_of_month,
      end_of_month: :date_end_of_month, at_end_of_month: :date_end_of_month,
      beginning_of_year: :date_beginning_of_year, at_beginning_of_year: :date_beginning_of_year,
      end_of_year: :date_end_of_year, at_end_of_year: :date_end_of_year,
      yesterday: :date_yesterday, tomorrow: :date_tomorrow
    }.freeze
    # Not next_day/prev_day/next_month/prev_month/next_year/prev_year: Date has its own, which Rails keeps.
    DATE_COUNTED = {
      days_since: :date_days_since, days_ago: :date_days_ago,
      weeks_since: :date_weeks_since, weeks_ago: :date_weeks_ago,
      months_since: :date_months_since, months_ago: :date_months_ago, last_month: :date_months_ago,
      years_since: :date_years_since, years_ago: :date_years_ago, last_year: :date_years_ago
    }.freeze
    DATE_NOW = { today?: :date_today?, yesterday?: :date_yesterday?, tomorrow?: :date_tomorrow?,
                 past?: :date_past?, future?: :date_future? }.freeze
    RANGE_UNITS = %i[month year].freeze
  end
end

class Time
  ActiveSupport::CalendarDispatch::TIME.each do |name, target|
    define_method(name) { ActiveSupport.public_send(target, self) }
  end
  ActiveSupport::CalendarDispatch::TIME_COUNTED.each do |name, target|
    define_method(name) { |n = 1| ActiveSupport.public_send(target, self, n) }
  end
  ActiveSupport::CalendarDispatch::TIME_NOW.each do |name|
    define_method(name) { ActiveSupport.public_send(name, self, ActiveSupport.now) }
  end

  %i[beginning_of_week at_beginning_of_week].each do |name|
    define_method(name) { |start = nil| ActiveSupport.beginning_of_week(self, ActiveSupport::CalendarDispatch.wday(start)) }
  end
  %i[end_of_week at_end_of_week].each do |name|
    define_method(name) { |start = nil| ActiveSupport.end_of_week(self, ActiveSupport::CalendarDispatch.wday(start)) }
  end

  def next_week(day = nil, same_time: false)
    ActiveSupport::CalendarDispatch.keep_time(self, ActiveSupport.days_since(ActiveSupport.next_week(self), ActiveSupport::CalendarDispatch.span(day)), same_time)
  end

  def prev_week(day = nil, same_time: false)
    ActiveSupport::CalendarDispatch.keep_time(self, ActiveSupport.days_since(ActiveSupport.prev_week(self), ActiveSupport::CalendarDispatch.span(day)), same_time)
  end
  alias_method :last_week, :prev_week

  def all_day
    beginning_of_day..end_of_day
  end

  def all_week(start = nil)
    beginning_of_week(start)..end_of_week(start)
  end

  ActiveSupport::CalendarDispatch::RANGE_UNITS.each do |unit|
    define_method(:"all_#{unit}") { public_send(:"beginning_of_#{unit}")..public_send(:"end_of_#{unit}") }
  end
end

if defined?(::Date)
  date_own = Date.instance_methods(false)
  class Date
    ActiveSupport::CalendarDispatch::DATE.each do |name, target|
      define_method(name) { ActiveSupport.public_send(target, self) }
    end
    ActiveSupport::CalendarDispatch::DATE_COUNTED.each do |name, target|
      define_method(name) { |n = 1| ActiveSupport.public_send(target, self, n) }
    end
    ActiveSupport::CalendarDispatch::DATE_NOW.each do |name, target|
      define_method(name) { ActiveSupport.public_send(target, self, ActiveSupport.date_current(ActiveSupport.now)) }
    end

    %i[beginning_of_week at_beginning_of_week].each do |name|
      define_method(name) { |start = nil| ActiveSupport.date_beginning_of_week(self, ActiveSupport::CalendarDispatch.wday(start)) }
    end
    %i[end_of_week at_end_of_week].each do |name|
      define_method(name) { |start = nil| ActiveSupport.date_end_of_week(self, ActiveSupport::CalendarDispatch.wday(start)) }
    end

    def next_week(day = nil)
      ActiveSupport.date_days_since(ActiveSupport.date_next_week(self), ActiveSupport::CalendarDispatch.span(day))
    end

    def prev_week(day = nil)
      ActiveSupport.date_days_since(ActiveSupport.date_prev_week(self), ActiveSupport::CalendarDispatch.span(day))
    end
    alias_method :last_week, :prev_week

    def all_week(start = nil)
      beginning_of_week(start)..end_of_week(start)
    end

    def all_day
      ActiveSupport.beginning_of_day(ActiveSupport.date_at_midnight(self))..ActiveSupport.end_of_day(ActiveSupport.date_at_midnight(self))
    end

    ActiveSupport::CalendarDispatch::RANGE_UNITS.each do |unit|
      define_method(:"all_#{unit}") { public_send(:"beginning_of_#{unit}")..public_send(:"end_of_#{unit}") }
    end

    def on_weekend?
      ActiveSupport.on_weekend?(ActiveSupport.date_at_midnight(self))
    end

    def on_weekday?
      ActiveSupport.on_weekday?(ActiveSupport.date_at_midnight(self))
    end
  end

  if defined?(::DateTime)
    calendar = Date.instance_methods(false) - date_own
    # Not inherited from the Date wrappers: the date_* helpers build a Date, dropping the clock and offset.
    calendar.each do |name|
      DateTime.define_method(name) { |*, **| raise NotImplementedError, "DateTime##{name} is not supported yet" }
    end
  end
end
