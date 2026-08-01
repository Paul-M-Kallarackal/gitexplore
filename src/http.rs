use async_graphql::ServerError;
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::{
    Extension, Json, Router,
    extract::{DefaultBodyLimit, Query, State},
    http::{
        HeaderMap, Method, StatusCode,
        header::{CONTENT_TYPE, COOKIE, ORIGIN, SET_COOKIE},
    },
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use serde::Deserialize;
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::{
    bookmarks::BookmarkTarget,
    bootstrap::AppState,
    exploration::ExplorationSeed,
    graphql::{GitExploreSchema, GraphQlSession, build_schema},
    shared::{AppError, ErrorEnvelope, Shared},
};

pub fn router(state: Shared<AppState>) -> Router {
    let schema = build_schema(state.clone());
    let canonical_frontend_origin = url::Url::parse(&state.frontend_origin)
        .expect("frontend origin must be validated before building the router")
        .origin()
        .ascii_serialization();
    let allow_origin = AllowOrigin::exact(
        canonical_frontend_origin
            .parse()
            .expect("validated frontend origin header"),
    );
    Router::new()
        .route("/health", get(health))
        .route("/auth/status", get(auth_status))
        .route("/auth/oauth/start", get(browser_start))
        .route("/auth/oauth/callback", get(browser_callback))
        .route("/auth/logout", post(browser_logout))
        .route("/graphql", post(graphql_handler))
        .route("/sync/run", post(run_sync))
        .route("/sync/status", get(sync_status))
        .route("/bookmarks", get(list_bookmarks).post(add_bookmark))
        .route("/categories", get(list_categories).post(create_category))
        .route("/explore", post(explore))
        .route("/explore/snapshots", get(exploration_snapshots))
        .layer(
            CorsLayer::new()
                .allow_origin(allow_origin)
                .allow_credentials(true)
                .allow_methods([Method::GET, Method::POST])
                .allow_headers([CONTENT_TYPE]),
        )
        .layer(DefaultBodyLimit::max(64 * 1024))
        .layer(Extension(schema))
        .with_state(state)
}

async fn health(State(state): State<Shared<AppState>>) -> impl IntoResponse {
    let graph_backend = match state.graph_backend {
        crate::config::GraphBackend::Memory => "memory",
        crate::config::GraphBackend::File => "file",
        crate::config::GraphBackend::Neo4j => "neo4j",
    };
    Json(serde_json::json!({
        "status": "ok",
        "graph_backend": graph_backend,
    }))
}

async fn graphql_handler(
    State(state): State<Shared<AppState>>,
    headers: HeaderMap,
    Extension(schema): Extension<GitExploreSchema>,
    request: GraphQLRequest,
) -> Response {
    let content_type_is_json = headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"));
    if !content_type_is_json {
        return (
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Json(serde_json::json!(ErrorEnvelope::message(
                "GraphQL requests require Content-Type: application/json",
            ))),
        )
            .into_response();
    }
    if let Some(origin) = headers.get(ORIGIN).and_then(|value| value.to_str().ok())
        && !same_origin(&state.frontend_origin, origin)
    {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!(ErrorEnvelope::message(
                "GraphQL request origin is not allowed",
            ))),
        )
            .into_response();
    }
    let user_id = match resolve_request_user(&state, &headers).await {
        Ok(user_id) => user_id,
        Err(error) => {
            return GraphQLResponse::from(async_graphql::Response::from_errors(vec![
                ServerError::new(error.to_string(), None),
            ]))
            .into_response();
        }
    };
    GraphQLResponse::from(
        schema
            .execute(request.into_inner().data(GraphQlSession { user_id }))
            .await,
    )
    .into_response()
}

async fn auth_status(
    State(state): State<Shared<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    match resolve_request_user(&state, &headers).await {
        Ok(Some(user_id)) => match state.services.identity.connection_status(&user_id).await {
            Ok(status) => (StatusCode::OK, Json(serde_json::json!(status))).into_response(),
            Err(error) => (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!(ErrorEnvelope::from(error))),
            )
                .into_response(),
        },
        Ok(None) => (
            StatusCode::OK,
            Json(serde_json::json!(crate::identity::ConnectionStatus {
                authenticated: false,
                app_user_id: None,
                connected: false,
                account: None,
            })),
        )
            .into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!(ErrorEnvelope::from(error))),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct BrowserStartQuery {
    redirect_to: Option<String>,
}

async fn browser_start(
    State(state): State<Shared<AppState>>,
    headers: HeaderMap,
    Query(query): Query<BrowserStartQuery>,
) -> impl IntoResponse {
    let existing_user = resolve_request_user(&state, &headers).await.ok().flatten();
    let user_id = existing_user.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let browser_nonce = uuid::Uuid::new_v4().to_string();
    let secure_cookies = origin_uses_https(&state.frontend_origin);
    let redirect_to = query
        .redirect_to
        .as_deref()
        .map(|requested| safe_frontend_redirect(&state.frontend_origin, Some(requested)));
    match state
        .services
        .identity
        .start_browser_login(&user_id, redirect_to, &browser_nonce)
        .await
    {
        Ok(url) => {
            let mut response = Redirect::temporary(&url).into_response();
            response.headers_mut().insert(
                SET_COOKIE,
                oauth_nonce_cookie_value(&browser_nonce, secure_cookies)
                    .parse()
                    .expect("OAuth nonce cookie header"),
            );
            response
        }
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!(ErrorEnvelope::from(error))),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: String,
    state: String,
}

async fn browser_callback(
    State(state): State<Shared<AppState>>,
    headers: HeaderMap,
    Query(query): Query<CallbackQuery>,
) -> impl IntoResponse {
    let secure_cookies = origin_uses_https(&state.frontend_origin);
    let Some(browser_nonce) = headers
        .get(COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookies| parse_cookie(cookies, "gitexplore_oauth_nonce"))
    else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!(ErrorEnvelope::message(
                "OAuth callback is not bound to this browser",
            ))),
        )
            .into_response();
    };
    match state
        .services
        .identity
        .complete_browser_login(&query.state, &query.code, &browser_nonce)
        .await
    {
        Ok(completed) => {
            if let Some(requested_redirect) = completed.redirect_to {
                let redirect_to =
                    safe_frontend_redirect(&state.frontend_origin, Some(&requested_redirect));
                let redirect = url::Url::parse(&redirect_to)
                    .map(|mut redirect| {
                        redirect.query_pairs_mut().append_pair("connected", "1");
                        redirect.to_string()
                    })
                    .unwrap_or(redirect_to);
                let mut response = Redirect::temporary(&redirect).into_response();
                response.headers_mut().insert(
                    SET_COOKIE,
                    session_cookie_value(&completed.session_id, secure_cookies)
                        .parse()
                        .expect("cookie header"),
                );
                response.headers_mut().append(
                    SET_COOKIE,
                    clear_oauth_nonce_cookie_value(secure_cookies)
                        .parse()
                        .expect("clear OAuth nonce cookie header"),
                );
                response
            } else {
                let mut response =
                    (StatusCode::OK, Json(serde_json::json!(completed.result))).into_response();
                response.headers_mut().insert(
                    SET_COOKIE,
                    session_cookie_value(&completed.session_id, secure_cookies)
                        .parse()
                        .expect("cookie header"),
                );
                response.headers_mut().append(
                    SET_COOKIE,
                    clear_oauth_nonce_cookie_value(secure_cookies)
                        .parse()
                        .expect("clear OAuth nonce cookie header"),
                );
                response
            }
        }
        Err(error) => {
            let mut response = (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!(ErrorEnvelope::from(error))),
            )
                .into_response();
            response.headers_mut().insert(
                SET_COOKIE,
                clear_oauth_nonce_cookie_value(secure_cookies)
                    .parse()
                    .expect("clear OAuth nonce cookie header"),
            );
            response
        }
    }
}

async fn browser_logout(
    State(state): State<Shared<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !mutation_origin_is_allowed(&state, &headers) {
        return forbidden_origin_response();
    }
    let secure_cookies = origin_uses_https(&state.frontend_origin);
    let session_id = headers
        .get(COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookies| parse_cookie(cookies, "gitexplore_session"));
    if let Some(session_id) = session_id
        && let Err(error) = state.services.identity.clear_session(&session_id).await
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!(ErrorEnvelope::from(error))),
        )
            .into_response();
    }
    let mut response = (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response();
    response.headers_mut().insert(
        SET_COOKIE,
        clear_session_cookie_value(secure_cookies)
            .parse()
            .expect("clear session cookie header"),
    );
    response
}

async fn run_sync(State(state): State<Shared<AppState>>, headers: HeaderMap) -> impl IntoResponse {
    if !mutation_origin_is_allowed(&state, &headers) {
        return forbidden_origin_response();
    }
    let Ok(Some(user_id)) = resolve_request_user(&state, &headers).await else {
        return unauthorized_response();
    };
    match state.services.sync.run_sync(&user_id).await {
        Ok(summary) => (StatusCode::OK, Json(serde_json::json!(summary))).into_response(),
        Err(error) => {
            let status = match &error {
                AppError::RateBudgetReserved { .. } => StatusCode::TOO_MANY_REQUESTS,
                AppError::GraphCapacityExceeded { .. } => StatusCode::INSUFFICIENT_STORAGE,
                _ => StatusCode::BAD_REQUEST,
            };
            (status, Json(serde_json::json!(ErrorEnvelope::from(error)))).into_response()
        }
    }
}

async fn sync_status(
    State(state): State<Shared<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let Ok(Some(user_id)) = resolve_request_user(&state, &headers).await else {
        return unauthorized_response();
    };
    match state.services.sync.status(&user_id).await {
        Ok(status) => (StatusCode::OK, Json(serde_json::json!(status))).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!(ErrorEnvelope::from(error))),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct CreateCategoryRequest {
    name: String,
    description: Option<String>,
}

async fn create_category(
    State(state): State<Shared<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<CreateCategoryRequest>,
) -> impl IntoResponse {
    if !mutation_origin_is_allowed(&state, &headers) {
        return forbidden_origin_response();
    }
    let Ok(Some(user_id)) = resolve_request_user(&state, &headers).await else {
        return unauthorized_response();
    };
    let category = crate::bookmarks::Category {
        name: payload.name,
        description: payload.description,
    };
    match state
        .services
        .bookmarks
        .create_category(&user_id, &category.name, category.description.clone())
        .await
    {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!(category))).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!(ErrorEnvelope::from(error))),
        )
            .into_response(),
    }
}

async fn list_categories(
    State(state): State<Shared<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let Ok(Some(user_id)) = resolve_request_user(&state, &headers).await else {
        return unauthorized_response();
    };
    match state.services.bookmarks.list_categories(&user_id).await {
        Ok(categories) => (StatusCode::OK, Json(serde_json::json!(categories))).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!(ErrorEnvelope::from(error))),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct AddBookmarkRequest {
    target: BookmarkTarget,
    categories: Vec<String>,
    note: Option<String>,
}

async fn add_bookmark(
    State(state): State<Shared<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<AddBookmarkRequest>,
) -> impl IntoResponse {
    if !mutation_origin_is_allowed(&state, &headers) {
        return forbidden_origin_response();
    }
    let Ok(Some(user_id)) = resolve_request_user(&state, &headers).await else {
        return unauthorized_response();
    };
    match state
        .services
        .bookmarks
        .add_bookmark(&user_id, payload.target, payload.categories, payload.note)
        .await
    {
        Ok(bookmark) => (StatusCode::OK, Json(serde_json::json!(bookmark))).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!(ErrorEnvelope::from(error))),
        )
            .into_response(),
    }
}

async fn list_bookmarks(
    State(state): State<Shared<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let Ok(Some(user_id)) = resolve_request_user(&state, &headers).await else {
        return unauthorized_response();
    };
    match state.services.bookmarks.list_bookmarks(&user_id).await {
        Ok(bookmarks) => (StatusCode::OK, Json(serde_json::json!(bookmarks))).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!(ErrorEnvelope::from(error))),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct ExploreRequest {
    seed_type: String,
    seed_value: String,
}

async fn explore(
    State(state): State<Shared<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<ExploreRequest>,
) -> impl IntoResponse {
    if !mutation_origin_is_allowed(&state, &headers) {
        return forbidden_origin_response();
    }
    let Ok(Some(user_id)) = resolve_request_user(&state, &headers).await else {
        return unauthorized_response();
    };
    let seed = match payload.seed_type.as_str() {
        "user" => Ok(ExplorationSeed::User {
            login: payload.seed_value,
        }),
        "repository" => Ok(ExplorationSeed::Repository {
            full_name: payload.seed_value,
        }),
        "category" => Ok(ExplorationSeed::Category {
            name: payload.seed_value,
        }),
        _ => Err(crate::shared::AppError::Validation(
            "seed_type must be one of user|repository|category".to_string(),
        )),
    };

    match seed.map(|seed| (seed, user_id)) {
        Ok((seed, user_id)) => match state.services.exploration.explore(&user_id, seed).await {
            Ok(result) => (StatusCode::OK, Json(serde_json::json!(result))).into_response(),
            Err(error) => (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!(ErrorEnvelope::from(error))),
            )
                .into_response(),
        },
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!(ErrorEnvelope::from(error))),
        )
            .into_response(),
    }
}

async fn exploration_snapshots(
    State(state): State<Shared<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let Ok(Some(user_id)) = resolve_request_user(&state, &headers).await else {
        return unauthorized_response();
    };
    match state.services.exploration.snapshots(&user_id).await {
        Ok(snapshots) => (StatusCode::OK, Json(serde_json::json!(snapshots))).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!(ErrorEnvelope::from(error))),
        )
            .into_response(),
    }
}

fn safe_frontend_redirect(frontend_origin: &str, requested: Option<&str>) -> String {
    let fallback = format!("{}/app", frontend_origin.trim_end_matches('/'));
    let Ok(frontend) = url::Url::parse(frontend_origin) else {
        return fallback;
    };
    let Some(requested) = requested else {
        return fallback;
    };
    let Ok(candidate) = url::Url::parse(requested) else {
        return fallback;
    };
    if urls_share_origin(&frontend, &candidate) {
        candidate.to_string()
    } else {
        fallback
    }
}

fn same_origin(configured_origin: &str, request_origin: &str) -> bool {
    let Ok(configured) = url::Url::parse(configured_origin) else {
        return false;
    };
    let Ok(request) = url::Url::parse(request_origin) else {
        return false;
    };
    urls_share_origin(&configured, &request)
}

fn urls_share_origin(left: &url::Url, right: &url::Url) -> bool {
    left.scheme() == right.scheme()
        && left.host_str() == right.host_str()
        && left.port_or_known_default() == right.port_or_known_default()
}

fn origin_uses_https(origin: &str) -> bool {
    url::Url::parse(origin).is_ok_and(|configured| configured.scheme() == "https")
}

fn mutation_origin_is_allowed(state: &AppState, headers: &HeaderMap) -> bool {
    headers
        .get(ORIGIN)
        .and_then(|value| value.to_str().ok())
        .is_none_or(|origin| same_origin(&state.frontend_origin, origin))
}

async fn resolve_request_user(
    state: &Shared<AppState>,
    headers: &HeaderMap,
) -> crate::shared::AppResult<Option<String>> {
    let Some(cookie_value) = headers.get(COOKIE).and_then(|value| value.to_str().ok()) else {
        return Ok(None);
    };
    let Some(session_id) = parse_cookie(cookie_value, "gitexplore_session") else {
        return Ok(None);
    };
    state.services.identity.resolve_session(&session_id).await
}

fn parse_cookie(header_value: &str, key: &str) -> Option<String> {
    header_value.split(';').map(str::trim).find_map(|part| {
        let (cookie_key, value) = part.split_once('=')?;
        (cookie_key == key).then(|| value.to_string())
    })
}

fn session_cookie_value(session_id: &str, secure: bool) -> String {
    format!(
        "gitexplore_session={session_id}; Path=/; HttpOnly; SameSite=Lax; Max-Age=2592000{}",
        secure_cookie_suffix(secure)
    )
}

fn clear_session_cookie_value(secure: bool) -> String {
    format!(
        "gitexplore_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{}",
        secure_cookie_suffix(secure)
    )
}

fn oauth_nonce_cookie_value(nonce: &str, secure: bool) -> String {
    format!(
        "gitexplore_oauth_nonce={nonce}; Path=/auth/oauth; HttpOnly; SameSite=Lax; Max-Age=600{}",
        secure_cookie_suffix(secure)
    )
}

fn clear_oauth_nonce_cookie_value(secure: bool) -> String {
    format!(
        "gitexplore_oauth_nonce=; Path=/auth/oauth; HttpOnly; SameSite=Lax; Max-Age=0{}",
        secure_cookie_suffix(secure)
    )
}

fn secure_cookie_suffix(secure: bool) -> &'static str {
    if secure { "; Secure" } else { "" }
}

fn unauthorized_response() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!(ErrorEnvelope::message(
            "authentication required",
        ))),
    )
        .into_response()
}

fn forbidden_origin_response() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!(ErrorEnvelope::message(
            "request origin is not allowed",
        ))),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::safe_frontend_redirect;

    #[test]
    fn oauth_redirects_stay_on_the_configured_frontend_origin() {
        let origin = "http://localhost:3000";

        assert_eq!(
            safe_frontend_redirect(origin, Some("http://localhost:3000/app/explore")),
            "http://localhost:3000/app/explore"
        );
        assert_eq!(
            safe_frontend_redirect(origin, Some("https://attacker.example/callback")),
            "http://localhost:3000/app"
        );
        assert_eq!(
            safe_frontend_redirect(origin, Some("http://localhost:4000/app")),
            "http://localhost:3000/app"
        );
    }
}
