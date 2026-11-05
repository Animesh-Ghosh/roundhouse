# frozen_string_literal: true
# Not in active_support_calendar: the date_* helpers ship only with the Date package.
# Not next_day/prev_day/next_month/prev_month/next_year/prev_year: Date has its own, which Rails keeps.
class Date
  def beginning_of_month
    ActiveSupport.date_beginning_of_month(self)
  end

  def at_beginning_of_month
    ActiveSupport.date_beginning_of_month(self)
  end

  def end_of_month
    ActiveSupport.date_end_of_month(self)
  end

  def at_end_of_month
    ActiveSupport.date_end_of_month(self)
  end

  def beginning_of_year
    ActiveSupport.date_beginning_of_year(self)
  end

  def at_beginning_of_year
    ActiveSupport.date_beginning_of_year(self)
  end

  def end_of_year
    ActiveSupport.date_end_of_year(self)
  end

  def at_end_of_year
    ActiveSupport.date_end_of_year(self)
  end

  def yesterday
    ActiveSupport.date_yesterday(self)
  end

  def tomorrow
    ActiveSupport.date_tomorrow(self)
  end

  def days_since(n = 1)
    ActiveSupport.date_days_since(self, n)
  end

  def days_ago(n = 1)
    ActiveSupport.date_days_ago(self, n)
  end

  def weeks_since(n = 1)
    ActiveSupport.date_weeks_since(self, n)
  end

  def weeks_ago(n = 1)
    ActiveSupport.date_weeks_ago(self, n)
  end

  def months_since(n = 1)
    ActiveSupport.date_months_since(self, n)
  end

  def months_ago(n = 1)
    ActiveSupport.date_months_ago(self, n)
  end

  def last_month(n = 1)
    ActiveSupport.date_months_ago(self, n)
  end

  def years_since(n = 1)
    ActiveSupport.date_years_since(self, n)
  end

  def years_ago(n = 1)
    ActiveSupport.date_years_ago(self, n)
  end

  def last_year(n = 1)
    ActiveSupport.date_years_ago(self, n)
  end

  def today?
    ActiveSupport.date_today?(self, ActiveSupport.date_current(ActiveSupport.now))
  end

  def yesterday?
    ActiveSupport.date_yesterday?(self, ActiveSupport.date_current(ActiveSupport.now))
  end

  def tomorrow?
    ActiveSupport.date_tomorrow?(self, ActiveSupport.date_current(ActiveSupport.now))
  end

  def past?
    ActiveSupport.date_past?(self, ActiveSupport.date_current(ActiveSupport.now))
  end

  def future?
    ActiveSupport.date_future?(self, ActiveSupport.date_current(ActiveSupport.now))
  end

  def beginning_of_week(start = nil)
    ActiveSupport.date_beginning_of_week(self, ActiveSupport::CalendarDispatch.wday(start))
  end

  def at_beginning_of_week(start = nil)
    ActiveSupport.date_beginning_of_week(self, ActiveSupport::CalendarDispatch.wday(start))
  end

  def end_of_week(start = nil)
    ActiveSupport.date_end_of_week(self, ActiveSupport::CalendarDispatch.wday(start))
  end

  def at_end_of_week(start = nil)
    ActiveSupport.date_end_of_week(self, ActiveSupport::CalendarDispatch.wday(start))
  end

  def next_week(day = nil)
    ActiveSupport.date_days_since(ActiveSupport.date_next_week(self), ActiveSupport::CalendarDispatch.span(day))
  end

  def prev_week(day = nil)
    ActiveSupport.date_days_since(ActiveSupport.date_prev_week(self), ActiveSupport::CalendarDispatch.span(day))
  end

  def last_week(day = nil)
    ActiveSupport.date_days_since(ActiveSupport.date_prev_week(self), ActiveSupport::CalendarDispatch.span(day))
  end

  def all_day
    ActiveSupport.beginning_of_day(ActiveSupport.date_at_midnight(self))..ActiveSupport.end_of_day(ActiveSupport.date_at_midnight(self))
  end

  def all_week(start = nil)
    beginning_of_week(start)..end_of_week(start)
  end

  def all_month
    beginning_of_month..end_of_month
  end

  def all_year
    beginning_of_year..end_of_year
  end

  def on_weekend?
    ActiveSupport.on_weekend?(ActiveSupport.date_at_midnight(self))
  end

  def on_weekday?
    ActiveSupport.on_weekday?(ActiveSupport.date_at_midnight(self))
  end
end

class DateTime
  # Not inherited from the Date wrappers above: the date_* helpers build a Date, dropping the clock and offset.
  def beginning_of_month(*_args, **_opts)
    raise NotImplementedError, "DateTime#beginning_of_month is not supported yet"
  end

  def at_beginning_of_month(*_args, **_opts)
    raise NotImplementedError, "DateTime#at_beginning_of_month is not supported yet"
  end

  def end_of_month(*_args, **_opts)
    raise NotImplementedError, "DateTime#end_of_month is not supported yet"
  end

  def at_end_of_month(*_args, **_opts)
    raise NotImplementedError, "DateTime#at_end_of_month is not supported yet"
  end

  def beginning_of_year(*_args, **_opts)
    raise NotImplementedError, "DateTime#beginning_of_year is not supported yet"
  end

  def at_beginning_of_year(*_args, **_opts)
    raise NotImplementedError, "DateTime#at_beginning_of_year is not supported yet"
  end

  def end_of_year(*_args, **_opts)
    raise NotImplementedError, "DateTime#end_of_year is not supported yet"
  end

  def at_end_of_year(*_args, **_opts)
    raise NotImplementedError, "DateTime#at_end_of_year is not supported yet"
  end

  def yesterday(*_args, **_opts)
    raise NotImplementedError, "DateTime#yesterday is not supported yet"
  end

  def tomorrow(*_args, **_opts)
    raise NotImplementedError, "DateTime#tomorrow is not supported yet"
  end

  def days_since(*_args, **_opts)
    raise NotImplementedError, "DateTime#days_since is not supported yet"
  end

  def days_ago(*_args, **_opts)
    raise NotImplementedError, "DateTime#days_ago is not supported yet"
  end

  def weeks_since(*_args, **_opts)
    raise NotImplementedError, "DateTime#weeks_since is not supported yet"
  end

  def weeks_ago(*_args, **_opts)
    raise NotImplementedError, "DateTime#weeks_ago is not supported yet"
  end

  def months_since(*_args, **_opts)
    raise NotImplementedError, "DateTime#months_since is not supported yet"
  end

  def months_ago(*_args, **_opts)
    raise NotImplementedError, "DateTime#months_ago is not supported yet"
  end

  def last_month(*_args, **_opts)
    raise NotImplementedError, "DateTime#last_month is not supported yet"
  end

  def years_since(*_args, **_opts)
    raise NotImplementedError, "DateTime#years_since is not supported yet"
  end

  def years_ago(*_args, **_opts)
    raise NotImplementedError, "DateTime#years_ago is not supported yet"
  end

  def last_year(*_args, **_opts)
    raise NotImplementedError, "DateTime#last_year is not supported yet"
  end

  def today?(*_args, **_opts)
    raise NotImplementedError, "DateTime#today? is not supported yet"
  end

  def yesterday?(*_args, **_opts)
    raise NotImplementedError, "DateTime#yesterday? is not supported yet"
  end

  def tomorrow?(*_args, **_opts)
    raise NotImplementedError, "DateTime#tomorrow? is not supported yet"
  end

  def past?(*_args, **_opts)
    raise NotImplementedError, "DateTime#past? is not supported yet"
  end

  def future?(*_args, **_opts)
    raise NotImplementedError, "DateTime#future? is not supported yet"
  end

  def beginning_of_week(*_args, **_opts)
    raise NotImplementedError, "DateTime#beginning_of_week is not supported yet"
  end

  def at_beginning_of_week(*_args, **_opts)
    raise NotImplementedError, "DateTime#at_beginning_of_week is not supported yet"
  end

  def end_of_week(*_args, **_opts)
    raise NotImplementedError, "DateTime#end_of_week is not supported yet"
  end

  def at_end_of_week(*_args, **_opts)
    raise NotImplementedError, "DateTime#at_end_of_week is not supported yet"
  end

  def next_week(*_args, **_opts)
    raise NotImplementedError, "DateTime#next_week is not supported yet"
  end

  def prev_week(*_args, **_opts)
    raise NotImplementedError, "DateTime#prev_week is not supported yet"
  end

  def last_week(*_args, **_opts)
    raise NotImplementedError, "DateTime#last_week is not supported yet"
  end

  def all_day(*_args, **_opts)
    raise NotImplementedError, "DateTime#all_day is not supported yet"
  end

  def all_week(*_args, **_opts)
    raise NotImplementedError, "DateTime#all_week is not supported yet"
  end

  def all_month(*_args, **_opts)
    raise NotImplementedError, "DateTime#all_month is not supported yet"
  end

  def all_year(*_args, **_opts)
    raise NotImplementedError, "DateTime#all_year is not supported yet"
  end

  def on_weekend?(*_args, **_opts)
    raise NotImplementedError, "DateTime#on_weekend? is not supported yet"
  end

  def on_weekday?(*_args, **_opts)
    raise NotImplementedError, "DateTime#on_weekday? is not supported yet"
  end
end
