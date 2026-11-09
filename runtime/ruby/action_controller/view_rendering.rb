# frozen_string_literal: true
# Rails' Base includes ActionView::Layouts, which includes this module;
# the responders gem asks `controller.class.include?(ActionView::Rendering)`
# before it renders a default template. Empty: Base renders views itself.
# A ruby-family reopen, as current.rb is: no strict target loads a gem
# that asks.
module ActionView
  module Rendering
  end
end

module ActionController
  class Base
    include ActionView::Rendering
  end
end
