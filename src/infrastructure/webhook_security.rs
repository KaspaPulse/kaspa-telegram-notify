use std::net::{IpAddr, SocketAddr};
use tokio_util::sync::CancellationToken;

pub fn env_bool_local(key: &str, default_value: bool) -> bool {
    std::env::var(key)
        .ok()
        .map(|value| {
            let value = value.trim().to_ascii_lowercase();
            value == "true" || value == "1" || value == "yes" || value == "on"
        })
        .unwrap_or(default_value)
}

pub fn env_u16_local(key: &str, default_value: u16) -> u16 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default_value)
}

pub fn validate_webhook_domain(domain: &str) -> Result<(), anyhow::Error> {
    let value = domain.trim();

    if value.is_empty() {
        return Err(anyhow::anyhow!("WEBHOOK_DOMAIN must not be empty"));
    }

    if value.starts_with("http://") || value.starts_with("https://") {
        return Err(anyhow::anyhow!(
            "WEBHOOK_DOMAIN must be a host only, without http:// or https://"
        ));
    }

    if value.contains('/') || value.contains('\\') || value.contains(' ') {
        return Err(anyhow::anyhow!(
            "WEBHOOK_DOMAIN must not contain paths, spaces, or slashes"
        ));
    }

    Ok(())
}

pub fn validate_webhook_secret(secret: &str) -> Result<(), anyhow::Error> {
    let value = secret.trim();

    if value.len() < 32 || value.len() > 256 {
        return Err(anyhow::anyhow!(
            "WEBHOOK_SECRET_TOKEN must be between 32 and 256 characters"
        ));
    }

    if !value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(anyhow::anyhow!(
            "WEBHOOK_SECRET_TOKEN may contain only A-Z, a-z, 0-9, '_' and '-'"
        ));
    }

    Ok(())
}

pub fn validate_webhook_bind_policy_value(
    app_env: &str,
    bind_ip: IpAddr,
    allow_public_bind: bool,
) -> Result<(), anyhow::Error> {
    if app_env.eq_ignore_ascii_case("production") && !bind_ip.is_loopback() && !allow_public_bind {
        return Err(anyhow::anyhow!(
            "WEBHOOK_BIND must stay on 127.0.0.1/loopback in production unless WEBHOOK_ALLOW_PUBLIC_BIND=true"
        ));
    }

    Ok(())
}

pub fn validate_webhook_bind_policy(app_env: &str, bind_ip: IpAddr) -> Result<(), anyhow::Error> {
    validate_webhook_bind_policy_value(
        app_env,
        bind_ip,
        env_bool_local("WEBHOOK_ALLOW_PUBLIC_BIND", false),
    )
}

pub fn webhook_max_connections() -> u8 {
    std::env::var("WEBHOOK_MAX_CONNECTIONS")
        .ok()
        .and_then(|value| value.parse::<u8>().ok())
        .filter(|value| (1..=100).contains(value))
        .unwrap_or(20)
}

pub fn validate_webhook_runtime_settings(
    app_env: &str,
    bind_ip: IpAddr,
    domain: &str,
    secret: &str,
) -> Result<(), anyhow::Error> {
    validate_webhook_domain(domain)?;
    validate_webhook_secret(secret)?;
    validate_webhook_bind_policy(app_env, bind_ip)?;

    Ok(())
}

pub fn spawn_health_endpoint(cancel_token: CancellationToken) {
    if !env_bool_local("HEALTH_ENDPOINT_ENABLED", true) {
        tracing::info!("[HEALTH] Health endpoint disabled by HEALTH_ENDPOINT_ENABLED=false");
        return;
    }

    let bind_raw = std::env::var("HEALTH_BIND").unwrap_or_else(|_| "127.0.0.1".to_string());
    let bind_ip: IpAddr = match bind_raw.parse() {
        Ok(value) => value,
        Err(error) => {
            tracing::error!(
                "[HEALTH] Invalid HEALTH_BIND value '{}': {}",
                bind_raw,
                error
            );
            return;
        }
    };

    if !bind_ip.is_loopback() && !env_bool_local("HEALTH_ALLOW_PUBLIC_BIND", false) {
        tracing::error!(
            "[HEALTH] Refusing to expose health endpoint on non-loopback address {}",
            bind_ip
        );
        return;
    }

    let port = env_u16_local("HEALTH_PORT", 18080);
    let addr = SocketAddr::new(bind_ip, port);

    crate::infrastructure::resilience::runtime::spawn_resilient("health_endpoint", async move {
        use axum::{Router, http::StatusCode, routing::get};

        async fn healthz() -> &'static str {
            "ok\n"
        }

        async fn readyz() -> (StatusCode, String) {
            let snapshot = crate::infrastructure::observability::snapshot();
            let state = snapshot.readiness(
                crate::infrastructure::metrics::now_unix_secs(),
                crate::infrastructure::observability::ReadinessPolicy::from_env(),
            );
            let status = match state {
                crate::infrastructure::observability::ReadinessState::Ready
                | crate::infrastructure::observability::ReadinessState::Degraded => StatusCode::OK,
                crate::infrastructure::observability::ReadinessState::NotReady => {
                    StatusCode::SERVICE_UNAVAILABLE
                }
            };

            (status, format!("{}\n", state.as_str()))
        }

        async fn metricsz() -> String {
            crate::infrastructure::metrics::render_metrics()
        }

        let app = Router::new()
            .route("/healthz", get(healthz))
            .route("/readyz", get(readyz))
            .route("/metrics", get(metricsz));

        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => listener,
            Err(error) => {
                tracing::error!(
                    "[HEALTH] Failed to bind health endpoint on {}: {}",
                    addr,
                    error
                );
                return;
            }
        };

        tracing::info!(
            "[HEALTH] Listening on {} with /healthz, /readyz and /metrics",
            addr
        );

        let shutdown = cancel_token.cancelled_owned();

        if let Err(error) = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                shutdown.await;
            })
            .await
        {
            tracing::error!("[HEALTH] Health endpoint failed: {}", error);
        }
    });
}
