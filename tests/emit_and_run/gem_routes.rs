//! A routing method a gem mixes into the mapper, as doorkeeper's `use_doorkeeper` is: its body draws
//! the routes, here from a block that configures it, and they answer where the call stands.

use super::emit_and_run;

const PING_ROUTES: &str = r##"module PingRoutes
  module Helper
    def use_pings(options = {}, &block)
      PingRoutes::Drawer.new(self, &block).draw(options[:path] || "pings")
    end
  end

  class Drawer
    def initialize(routes, &block)
      @routes = routes
      @controller = "pings"
      instance_eval(&block) if block
    end

    def controller(name)
      @controller = name
    end

    def draw(path)
      @routes.scope path, as: "pings" do
        @routes.get "/", controller: @controller, action: :show, as: :show
      end
    end
  end

  def self.install!
    ActionDispatch::Routing::Mapper.include PingRoutes::Helper
  end
end
"##;

#[test]
fn a_gems_routing_method_draws_routes_that_answer() {
    emit_and_run::real_blog()
        .write("lib/ping_routes.rb", PING_ROUTES)
        .write(
            "app/controllers/pongs_controller.rb",
            "class PongsController < ApplicationController\n  def show\n    render plain: \"pong #{pings_show_path}\"\n  end\nend\n",
        )
        .edit(
            "config/routes.rb",
            "  root \"articles#index\"\n",
            "  root \"articles#index\"\n  use_pings path: \"ping\" do\n    controller \"pongs\"\n  end\n",
        )
        .write(
            "test/controllers/pongs_controller_test.rb",
            "require \"test_helper\"\n\nclass PongsControllerTest < ActionDispatch::IntegrationTest\n  test \"the drawn route answers\" do\n    get \"/ping\"\n    assert_response :success\n    assert_equal \"pong /ping\", response.body\n  end\nend\n",
        )
        .run_test("test/controllers/pongs_controller_test.rb")
        .assert_passes();
}
