# frozen_string_literal: true
# Ruby's `SimpleDelegator`, the part the Spinel tree can compile: it holds
# the object it wraps and hands it back (`__getobj__`, `__setobj__`), so a
# gem's subclass loads and its own methods run (rack-attack's store
# proxies). Not the forwarding: Ruby's sends every other call on through
# `method_missing` by its runtime name, which Spinel cannot compile, so such
# a call stays the refusal Spinel reports. The ruby family swaps this file
# for `require "delegate"` (`project::ruby_runtime_files`).
class Delegator
  def initialize(obj)
    __setobj__(obj)
  end
end

class SimpleDelegator < Delegator
  def __getobj__
    @delegate_sd_obj
  end

  def __setobj__(obj)
    @delegate_sd_obj = obj
  end
end
