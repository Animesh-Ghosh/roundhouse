# Shared by the Rails date-column control and the actual adapter probes.
class InheritedQuoteDate < Date
end

class WrittenQuoteDate < Date
  def strftime(pattern)
    "written'" + super(pattern)
  end
end

class InheritedWrittenQuoteDate < WrittenQuoteDate
end

module QuoteDateFormatting
  def strftime(pattern)
    "included'" + super(pattern)
  end
end

class IncludedQuoteDate < Date
  include QuoteDateFormatting
end

puts quote_date_value(Date.new(2024, 2, 29))
puts quote_date_value(DateTime.new(2024, 3, 1, 0, 30, 0, '+14:00'))
puts quote_date_value(DateTime.new(2024, 2, 29, 23, 30, 0, '-12:00'))
puts quote_date_value(Date.new(-1, 1, 2))
puts quote_date_value(Date.new(10000, 1, 2))
puts quote_date_value(InheritedQuoteDate.new(2024, 2, 29))
puts quote_date_value(WrittenQuoteDate.new(2024, 2, 29))
puts quote_date_value(InheritedWrittenQuoteDate.new(2024, 2, 29))
puts quote_date_value(IncludedQuoteDate.new(2024, 2, 29))
