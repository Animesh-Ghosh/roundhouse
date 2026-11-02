# frozen_string_literal: true

# The raw Faye subset over tep's RFC6455 transport. There is no reactor:
# native Driver writes are serialized and schedule runs on the caller.
module EventMachine
  def self.schedule
    yield
  end
end

module Faye
  class WebSocket
    class Callback < Tep::WebSocket::Handler
      def initialize(&callback)
        @callback = callback
      end

      def handle_event(event)
        # The upgrade request's lease ends before the frame loop starts.
        Db.with_connection { @callback.call(event) }
        0
      end
    end

    # Lifecycle handlers remain installed even when an app registers its
    # own open/close callbacks, so heartbeat threads always get stopped.
    class Lifecycle < Tep::WebSocket::Handler
      def initialize(socket, opening)
        @socket = socket
        @opening = opening
      end

      def handle_event(event)
        if @opening
          @socket.opened(event)
        else
          @socket.closed(event)
        end
        0
      end
    end

    def self.request_from_env(env)
      req = Tep::Request.new
      req.verb = env["REQUEST_METHOD"].to_s
      req.req_headers["upgrade"] = env["HTTP_UPGRADE"].to_s
      req.req_headers["connection"] = env["HTTP_CONNECTION"].to_s
      req.req_headers["sec-websocket-version"] = env["HTTP_SEC_WEBSOCKET_VERSION"].to_s
      req.req_headers["sec-websocket-key"] = env["HTTP_SEC_WEBSOCKET_KEY"].to_s
      req.req_headers["sec-websocket-protocol"] = env["HTTP_SEC_WEBSOCKET_PROTOCOL"].to_s
      req
    end

    def self.websocket?(env)
      return false unless env["HTTP_UPGRADE"].to_s.downcase == "websocket"
      return false unless env["HTTP_CONNECTION"].to_s.downcase.split(",").any? { |value| value.strip == "upgrade" }
      Tep::WebSocket::Handshake.check(request_from_env(env)).valid
    end

    attr_reader :driver, :accept_key

    def initialize(env, protocols = nil, ping: 0)
      raise ArgumentError, "Faye subprotocols are not supported on Spinel" unless protocols.nil? || (protocols.is_a?(Array) && protocols.empty?)
      raise ArgumentError, "Faye ping must be nonnegative" unless ping >= 0
      raise ArgumentError, "not a WebSocket upgrade" unless WebSocket.websocket?(env)
      @env = env
      @accept_key = Tep::WebSocket::Handshake.check(WebSocket.request_from_env(env)).accept_key
      @driver = Tep::WebSocket::Driver.new(-1)
      @ping_interval = ping
      @ping_threads = []
      @registered = []
      @on_open = Tep::WebSocket::Handler.new
      @on_close = Tep::WebSocket::Handler.new
      @driver.set_on_open(Lifecycle.new(self, true))
      @driver.set_on_close(Lifecycle.new(self, false))
    end

    def on(type, &callback)
      raise ArgumentError, "Faye callback block required" if callback.nil?
      raise ArgumentError, "duplicate Faye callback is not supported" if @registered.include?(type)
      handler = Callback.new(&callback)
      case type
      when :message then @driver.set_on_message(handler)
      when :close then @on_close = handler
      when :open then @on_open = handler
      else raise ArgumentError, "unsupported WebSocket event"
      end
      @registered.push(type)
      self
    end

    def opened(event)
      if @ping_interval > 0
        timer = Thread.new(@driver, @ping_interval) do |driver, interval|
          loop do
            sleep interval
            break if driver.ping("") < 0
          end
        end
        @ping_threads.push(timer)
      end
      @on_open.handle_event(event)
      0
    end

    def closed(event)
      @ping_threads.each { |timer| timer.kill }
      @ping_threads.clear
      @on_close.handle_event(event)
      0
    end

    def send(data)
      raise ArgumentError, "Faye binary messages are not supported on Spinel" unless data.is_a?(String)
      @driver.text(data) >= 0
    end

    def close(code = 1000, reason = "")
      raise ArgumentError, "unsupported Faye close code" unless code == 1000 || (code >= 3000 && code <= 4999)
      raise ArgumentError, "Faye close reason exceeds 123 bytes" if reason.bytesize > 123
      @driver.close(code, reason)
    end

    def rack_response
      @env["roundhouse.websocket"] = self
      [ -1, {}, [] ]
    end
  end
end
