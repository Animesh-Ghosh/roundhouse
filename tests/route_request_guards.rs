//! A request constraint, a lambda or an object Rails asks with the
//! request (`constraints: ->(req) { req.format == :json }`,
//! `constraints(SomeGuard.new) do … end`), guards its routes. The router
//! matches by verb and path alone, so it cannot check one: the routes it
//! guards are left out of the dispatch table and the constraint is an
//! error where it is written, and a request reaches the routes after
//! them. Serving them anyway answered every request the constraint
//! refuses, which left a later route on the same path unreachable (#727).

use roundhouse::App;
use roundhouse::ingest::ingest_routes;
use roundhouse::lower::routes::{flatten_routes, flatten_routes_for_helpers};

fn app(source: &str) -> App {
    let mut app = App::default();
    app.routes = ingest_routes(format!("Rails.application.routes.draw do\n{source}\nend\n").as_bytes(), "config/routes.rb")
        .expect("ingest routes");
    app
}

#[test]
fn a_lambda_constraint_leaves_its_route_out_and_the_next_one_answers() {
    let app = app(
        r#"get "/invite/:code", to: "widgets#show", constraints: ->(req) { req.format == :json }
get "/invite/:code", to: "gadgets#new""#,
    );
    let dispatched: Vec<(String, String)> = flatten_routes(&app).iter().map(|r| (r.path.clone(), r.controller.0.as_str().to_string())).collect();
    assert_eq!(dispatched, [("/invite/:code".to_string(), "GadgetsController".to_string())], "{dispatched:?}");
    let errors: Vec<_> = app.routes.diagnostics.iter().filter(|d| d.message.contains("request constraint")).collect();
    assert_eq!(errors.len(), 1, "{:#?}", app.routes.diagnostics);
    assert_eq!(errors[0].severity, roundhouse::diagnostic::Severity::Error);
}

#[test]
fn a_guard_object_block_refuses_every_route_inside_and_keeps_their_helpers() {
    let app = app(
        r#"constraints(AdminGuard.new) do
  get "/reports", to: "reports#index", as: :reports
  resources :audits, only: :index
end
get "/ping", to: "pings#show""#,
    );
    let flat = flatten_routes(&app);
    let dispatched: Vec<&str> = flat.iter().map(|r| r.path.as_str()).collect();
    assert_eq!(dispatched, ["/ping"], "{dispatched:?}");
    let helpers: Vec<String> = flatten_routes_for_helpers(&app).iter().map(|r| r.as_name.clone()).collect();
    assert!(helpers.iter().any(|h| h == "reports") && helpers.iter().any(|h| h == "audits"), "{helpers:?}");
    let errors = app.routes.diagnostics.iter().filter(|d| d.message.contains("request constraint")).count();
    assert_eq!(errors, 1, "one error at the constraint: {:#?}", app.routes.diagnostics);
}
