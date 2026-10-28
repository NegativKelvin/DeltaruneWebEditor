//! Request checks for a server that only the local webpage may use.

use axum::extract::Request;
use axum::http::{Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

fn is_loopback_name(name: &str) -> bool {
    matches!(name, "localhost" | "127.0.0.1" | "[::1]")
}

/// Host name of `host[:port]`.
fn host_name(host: &str) -> &str {
    if host.starts_with('[') {
        // [::1]:7878 or [::1]
        return host.find(']').map_or(host, |i| &host[..=i]);
    }
    host.rsplit_once(':').map_or(host, |(n, _)| n)
}

/// Blocks DNS-rebinding: only accept requests addressed to a loopback host name.
pub async fn local_host_only(req: Request, next: Next) -> Response {
    let host = req.headers().get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("");
    if is_loopback_name(host_name(host)) {
        next.run(req).await
    } else {
        (StatusCode::FORBIDDEN, "host not allowed").into_response()
    }
}

/// The request comes from a page of another origin (another site, or another port on this computer).
/// Browsers send `Origin` on POST/PUT/DELETE (also for "simple" requests that skip the CORS preflight),
/// and `Sec-Fetch-Site` on all requests. Tools such as curl send neither.
fn is_cross_origin(req: &Request) -> bool {
    let h = req.headers();
    if h.get("sec-fetch-site")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v == "cross-site" || v == "same-site")
    {
        return true;
    }
    let Some(origin) = h.get(header::ORIGIN) else {
        return false;
    };
    let host = h.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("");
    let authority = origin
        .to_str()
        .ok()
        .and_then(|o| o.strip_prefix("http://").or_else(|| o.strip_prefix("https://")));
    authority != Some(host) || !is_loopback_name(host_name(host))
}

/// Blocks cross-site request forgery: a web page on another site must not change files or start jobs.
/// Reads (GET, HEAD) stay open, because the browser does not let that page read the answer.
pub async fn same_site_writes(req: Request, next: Next) -> Response {
    let read = matches!(*req.method(), Method::GET | Method::HEAD);
    if !read && is_cross_origin(&req) {
        (StatusCode::FORBIDDEN, "cross-site request not allowed").into_response()
    } else {
        next.run(req).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::body::Body;
    use axum::routing::post;
    use tower::ServiceExt;

    fn app() -> Router {
        Router::new()
            .route("/x", post(|| async { "ok" }).get(|| async { "ok" }))
            .layer(axum::middleware::from_fn(same_site_writes))
            .layer(axum::middleware::from_fn(local_host_only))
    }

    async fn status(method: &str, host: &str, headers: &[(&str, &str)]) -> StatusCode {
        let mut req = Request::builder().method(method).uri("/x").header("host", host);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        app().oneshot(req.body(Body::empty()).unwrap()).await.unwrap().status()
    }

    #[tokio::test]
    async fn host_names() {
        assert_eq!(host_name("localhost:7878"), "localhost");
        assert_eq!(host_name("[::1]:7878"), "[::1]");
        assert_eq!(host_name("[::1]"), "[::1]");
        assert_eq!(status("GET", "127.0.0.1:7878", &[]).await, StatusCode::OK);
        assert_eq!(status("GET", "[::1]:7878", &[]).await, StatusCode::OK);
        assert_eq!(status("GET", "evil.example", &[]).await, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn blocks_cross_site_writes_only() {
        let local = [("origin", "http://127.0.0.1:7878")];
        assert_eq!(status("POST", "127.0.0.1:7878", &local).await, StatusCode::OK);
        assert_eq!(
            status("POST", "localhost:7878", &[("origin", "http://localhost:7878")]).await,
            StatusCode::OK
        );
        assert_eq!(status("POST", "localhost", &[]).await, StatusCode::OK); // curl, tests
        let evil = [("origin", "https://evil.example")];
        assert_eq!(status("POST", "127.0.0.1:7878", &evil).await, StatusCode::FORBIDDEN);
        assert_eq!(status("POST", "127.0.0.1:7878", &[("origin", "null")]).await, StatusCode::FORBIDDEN);
        assert_eq!(
            status("POST", "127.0.0.1:7878", &[("origin", "http://localhost.evil.example")]).await,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            status("POST", "127.0.0.1:7878", &[("sec-fetch-site", "cross-site")]).await,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            status("POST", "127.0.0.1:7878", &[("origin", "http://127.0.0.1:3000")]).await,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            status(
                "PUT",
                "127.0.0.1:7878",
                &[("origin", "http://127.0.0.1:7878"), ("sec-fetch-site", "same-origin")]
            )
            .await,
            StatusCode::METHOD_NOT_ALLOWED
        );
        assert_eq!(status("GET", "127.0.0.1:7878", &evil).await, StatusCode::OK);
    }
}
