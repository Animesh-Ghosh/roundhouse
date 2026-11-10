//! `respond_with` from the responders gem, for a controller whose class-level `respond_to` is json
//! alone: what the Responder's `to_format` path answers. The expected table is the answer of the
//! real gem (responders 3.2.1 under Rails 8.1) to the same requests.

use super::emit_and_run;

const CONTROLLER: &str = r#"class GadgetsController < ApplicationController
  skip_forgery_protection
  respond_to :json

  def show
    @article = Article.new(title: "s", body: "a body long enough to pass")
    respond_with @article
  end

  def tmpl
    @article = Article.new(title: params[:name], body: "a body long enough to pass")
    @article.valid?
    respond_with @article
  end

  def create
    @article = Article.new(title: params[:name], body: "a body long enough to pass")
    @article.valid?
    respond_with @article, location: "/done"
  end

  def empty
    respond_with({}, location: "/sent")
  end

  def update
    @article = Article.new(title: params[:name], body: "a body long enough to pass")
    @article.valid?
    respond_with @article, location: "/done"
  end

  def destroy
    @article = Article.new(title: params[:name], body: "a body long enough to pass")
    @article.valid?
    respond_with @article, location: "/done"
  end

  def stat
    @article = Article.new(title: "x", body: "a body long enough to pass")
    respond_with @article, status: :accepted, location: "/done"
  end

  def typed
    @article = Article.new(title: params[:name], body: "a body long enough to pass")
    @article.valid?
    respond_with @article, location: "/done", content_type: "application/vnd.gadget+json"
  end

  def blk
    @article = Article.new(title: params[:name], body: "a body long enough to pass")
    @article.valid?
    respond_with(@article, location: "/done") { |format| format.html { render plain: "custom" } }
  end

  def act
    @article = Article.new
    @article.valid?
    respond_with @article, action: :edit, location: "/done"
  end

  def several
    @article = Article.new(title: "x", body: "a body long enough to pass")
    @other = Article.new(title: "y", body: "a body long enough to pass")
    respond_with @article, @other
  end
end
"#;

const ROUTES: &str = r#"  get "g/show", to: "gadgets#show"
  get "g/tmpl", to: "gadgets#tmpl"
  post "g/tmpl", to: "gadgets#tmpl"
  post "g/create", to: "gadgets#create"
  post "g/empty", to: "gadgets#empty"
  put "g/update", to: "gadgets#update"
  patch "g/update", to: "gadgets#update"
  delete "g/destroy", to: "gadgets#destroy"
  post "g/stat", to: "gadgets#stat"
  post "g/blk", to: "gadgets#blk"
  post "g/typed", to: "gadgets#typed"
  post "g/act", to: "gadgets#act"
  post "g/several", to: "gadgets#several"
"#;

#[test]
fn respond_with_answers_as_the_responder_does_for_json() {
    let run = emit_and_run::real_blog()
        .write("app/controllers/gadgets_controller.rb", CONTROLLER)
        .write("app/views/gadgets/tmpl.json.jbuilder", "json.tmpl @article.title\n")
        .edit("config/routes.rb", "  root \"articles#index\"\n", &format!("  root \"articles#index\"\n{ROUTES}"))
        .run_ruby(
            r##"
def hit(verb, path, query = "", accept = "application/json")
  env = { "REQUEST_METHOD" => verb, "PATH_INFO" => path, "QUERY_STRING" => query, "REMOTE_ADDR" => "127.0.0.1",
          "HTTP_HOST" => "localhost", "rack.input" => StringIO.new(""), "HTTP_ACCEPT" => accept }
  status, headers, body = Main.run_rack(env)
  [status, headers["location"] || headers["Location"], body.join.strip]
end
# verb, path, query, accept, status, location, body substring
[
  ["GET",    "/g/show",    "",       "json", 200, nil,     "\"title\":\"s\""],
  ["GET",    "/g/tmpl",    "name=t", "json", 200, nil,     "{\"tmpl\":\"t\"}"],
  ["POST",   "/g/tmpl",    "name=t", "json", 200, nil,     "{\"tmpl\":\"t\"}"],
  ["POST",   "/g/tmpl",    "",       "json", 422, nil,     "{\"errors\":{\"title\":"],
  ["POST",   "/g/create",  "name=a", "json", 201, "/done", "\"title\":\"a\""],
  ["POST",   "/g/create",  "",       "json", 422, nil,     "{\"errors\":{\"title\":"],
  ["POST",   "/g/empty",   "",       "json", 201, "/sent", "{}"],
  ["PUT",    "/g/update",  "name=a", "json", 204, "/done", ""],
  ["PATCH",  "/g/update",  "name=a", "json", 204, "/done", ""],
  ["PUT",    "/g/update",  "",       "json", 422, nil,     "{\"errors\":{\"title\":"],
  ["DELETE", "/g/destroy", "name=a", "json", 204, "/done", ""],
  ["DELETE", "/g/destroy", "",       "json", 422, nil,     "{\"errors\":{\"title\":"],
  ["POST",   "/g/stat",    "",       "json", 202, "/done", "\"title\":\"x\""],
  ["POST",   "/g/blk",     "name=a", "json", 201, "/done", "\"title\":\"a\""],
  ["GET",    "/g/show",    "",       "html", 406, nil,     ""],
  ["POST",   "/g/blk",     "name=a", "html", 200, nil,     "custom"],
].each do |verb, path, query, accept, status, location, body|
  # The native dispatchers take a request's format from the path's extension, not from Accept.
  got = hit(verb, accept == "json" ? path + ".json" : path, query, accept == "json" ? "application/json" : "text/html")
  ok = got[0] == status && got[1].to_s.sub("http://localhost", "") == location.to_s && got[2].include?(body)
  raise "#{verb} #{path}?#{query} (#{accept}): got #{got.inspect}, want #{[status, location, body].inspect}" unless ok
end
# The call's other options reach the resource's render, not the errors' (`display_errors`).
[["name=a", 201, "application/vnd.gadget+json"], ["", 422, "application/json"]].each do |query, status, type|
  env = { "REQUEST_METHOD" => "POST", "PATH_INFO" => "/g/typed.json", "QUERY_STRING" => query, "REMOTE_ADDR" => "127.0.0.1",
          "HTTP_HOST" => "localhost", "rack.input" => StringIO.new(""), "HTTP_ACCEPT" => "application/json" }
  got_status, headers, _ = Main.run_rack(env)
  got_type = (headers["content-type"] || headers["Content-Type"]).to_s.split(";").first
  raise "typed?#{query}: got #{[got_status, got_type].inspect}, want #{[status, type].inspect}" unless got_status == status && got_type == type
end
# Not modeled: these must fail, not answer with a response Rails wouldn't give.
["/g/act", "/g/several"].each do |path|
  got = begin
    hit("POST", path + ".json")
  rescue ArgumentError, NoMethodError, NameError
    [500]
  end
  raise "#{path}: expected a failure, got #{got.inspect}" unless got[0] == 500
end
puts "respond_with json passed"
"##,
        );
    run.assert_passes();
    assert!(run.stdout.contains("respond_with json passed"), "{}", run.stdout);
}
