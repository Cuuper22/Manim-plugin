//! HTTP §2 and §14 items 1–12: credentials, bootstrap, Host and Origin.

use super::*;

fn bare(method: Method, path: &str, host: Option<&str>) -> Builder {
    let builder = Request::builder().method(method).uri(path);
    match host {
        Some(host) => builder.header(header::HOST, host),
        None => builder,
    }
}

#[tokio::test]
async fn api_routes_need_the_token_as_bearer_or_cookie() {
    let harness = Harness::new("").await;
    let anonymous = harness
        .send(
            bare(Method::GET, "/api/health", Some(HOST))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(anonymous.error(StatusCode::UNAUTHORIZED), "unauthorized");
    assert_eq!(
        anonymous.header(header::WWW_AUTHENTICATE),
        r#"Bearer realm="manim-director""#
    );
    assert_eq!(anonymous.json()["error"]["data"], Value::Null);
    let wrong = bare(Method::GET, "/api/health", Some(HOST))
        .header(header::AUTHORIZATION, "Bearer not-the-token")
        .body(Body::empty())
        .unwrap();
    assert_eq!(harness.send(wrong).await.status, StatusCode::UNAUTHORIZED);
    let unknown_route = bare(Method::GET, "/api/nope", Some(HOST))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        harness.send(unknown_route).await.status,
        StatusCode::UNAUTHORIZED
    );

    let health = harness.get("/api/health").await;
    assert_eq!(health.status, StatusCode::OK);
    let body = health.json();
    assert_eq!(body["api_version"], 2);
    assert_eq!(
        body["instance_id"],
        harness.state.scheduler.instance_id().to_string()
    );
    assert_eq!(health.header(header::CACHE_CONTROL), "no-store");
    assert_eq!(health.header(header::X_CONTENT_TYPE_OPTIONS), "nosniff");
    assert_eq!(health.header(header::REFERRER_POLICY), "no-referrer");

    let cookie = bare(Method::GET, "/api/health", Some(HOST))
        .header(
            header::COOKIE,
            format!("theme=dark; mdsess_4177={}", harness.token()),
        )
        .body(Body::empty())
        .unwrap();
    assert_eq!(harness.send(cookie).await.status, StatusCode::OK);
    let other_port = bare(Method::GET, "/api/health", Some(HOST))
        .header(header::COOKIE, format!("mdsess_9999={}", harness.token()))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        harness.send(other_port).await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn the_page_needs_no_token_and_bootstraps_the_cookie() {
    let harness = Harness::new("").await;
    let page = harness
        .send(
            bare(Method::GET, "/", Some(HOST))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(page.status, StatusCode::OK);
    assert_eq!(
        page.header(header::CONTENT_TYPE),
        "text/html; charset=utf-8"
    );
    assert_eq!(page.header(header::CACHE_CONTROL), "no-cache");
    assert!(page
        .header(header::CONTENT_SECURITY_POLICY)
        .contains("frame-ancestors 'none'"));
    let spa_route = harness
        .send(
            bare(Method::GET, "/scenes/x", Some(HOST))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(
        spa_route.header(header::CONTENT_TYPE),
        "text/html; charset=utf-8"
    );

    let path = format!("/?token={}", harness.token());
    let signed_in = harness
        .send(
            bare(Method::GET, &path, Some(HOST))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(signed_in.status, StatusCode::SEE_OTHER);
    assert_eq!(signed_in.header(header::LOCATION), "/");
    assert_eq!(signed_in.header(header::CACHE_CONTROL), "no-store");
    assert_eq!(signed_in.header(header::REFERRER_POLICY), "no-referrer");
    let cookie = signed_in.header(header::SET_COOKIE);
    assert!(cookie.starts_with(&format!("mdsess_4177={};", harness.token())));
    assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Strict"));

    let refused = harness
        .send(
            bare(Method::GET, "/?token=forged", Some(HOST))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(refused.status, StatusCode::SEE_OTHER);
    assert!(refused.headers.get(header::SET_COOKIE).is_none());
}

#[tokio::test]
async fn hosts_other_than_this_loopback_server_are_refused() {
    let harness = Harness::new("").await;
    for host in ["evil.com:4177", "localhost:9999", "localhost.:4177"] {
        let reply = harness
            .send(
                bare(Method::GET, "/", Some(host))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
        assert_eq!(
            reply.error(StatusCode::FORBIDDEN),
            "forbidden_host",
            "{host}"
        );
        assert_eq!(reply.json()["error"]["data"]["host"], host);
    }
    let missing = harness
        .send(
            bare(Method::GET, "/api/health", None)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(missing.error(StatusCode::FORBIDDEN), "forbidden_host");
    let twice = bare(Method::GET, "/", Some(HOST))
        .header(header::HOST, "localhost:4177")
        .body(Body::empty())
        .unwrap();
    assert_eq!(harness.send(twice).await.status, StatusCode::FORBIDDEN);
    for host in ["localhost:4177", "[::1]:4177"] {
        let reply = harness
            .send(
                bare(Method::GET, "/", Some(host))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
        assert_eq!(reply.status, StatusCode::OK, "{host}");
    }

    let remote = Harness::with(
        "",
        Options {
            allow_remote: true,
            ..Options::default()
        },
    )
    .await;
    let anywhere = remote
        .send(
            bare(Method::GET, "/", Some("evil.com:4177"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(anywhere.status, StatusCode::OK);
}

#[tokio::test]
async fn writes_need_the_exact_origin_and_a_json_body() {
    let harness = Harness::new("").await;
    let job = harness
        .json(
            Method::POST,
            "/api/jobs",
            &serde_json::json!({"operation": "doctor"}),
        )
        .await;
    assert_eq!(job.status, StatusCode::ACCEPTED);
    let cancel = format!("/api/jobs/{}/cancel", job.json()["id"].as_str().unwrap());
    for origin in ["https://evil.com", "http://localhost:3000", "null"] {
        let request = harness
            .request(Method::POST, &cancel)
            .header(header::ORIGIN, origin)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from("{}"))
            .unwrap();
        let reply = harness.send(request).await;
        assert_eq!(
            reply.error(StatusCode::FORBIDDEN),
            "forbidden_origin",
            "{origin}"
        );
    }
    let same_origin = harness
        .request(Method::POST, &cancel)
        .header(header::ORIGIN, "http://127.0.0.1:4177")
        .header(header::CONTENT_TYPE, "application/json; charset=utf-8")
        .body(Body::from("{}"))
        .unwrap();
    assert!(harness.send(same_origin).await.status.is_success());
    let no_type = harness
        .request(Method::POST, &cancel)
        .body(Body::from("{}"))
        .unwrap();
    let reply = harness.send(no_type).await;
    assert_eq!(
        reply.error(StatusCode::UNSUPPORTED_MEDIA_TYPE),
        "unsupported_media_type"
    );
    let form = harness
        .request(Method::POST, &cancel)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(Body::from("{}"))
        .unwrap();
    assert_eq!(
        harness.send(form).await.status,
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
}
