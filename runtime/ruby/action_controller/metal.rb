# frozen_string_literal: true
# ActionController::Metal and the modules a Metal subclass mixes back in
# (devise's FailureApp: `class FailureApp < ActionController::Metal;
# include ActionController::UrlFor, ActionController::Redirecting,
# AbstractController::Callbacks`). Rails builds Base from Metal and these
# modules; here Base already holds what they give, so Metal is Base under
# Metal's name, as API is (api.rb), and the modules are empty.
module AbstractController
  module Callbacks
  end
end

module ActionController
  module UrlFor
  end

  module Redirecting
  end

  class Metal < Base
    # What Rails' AbstractController::Callbacks registers, so a Metal subclass's body loads (Devise's FailureApp,
    # which the boot names). Not run: a Metal action is reached through `action(:name)`, which is not modeled.
    def self.around_action(*names, &block)
      nil
    end

    def self.before_action(*names, &block)
      nil
    end

    def self.after_action(*names, &block)
      nil
    end
  end
end
