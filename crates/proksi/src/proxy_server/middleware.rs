use std::collections::HashMap;

use pingora::Result;

use crate::plugins::MiddlewarePlugin;

/// Executes the request and response plugins
pub async fn execute_response_plugins(
    session: &mut pingora::proxy::Session,
    ctx: &mut crate::proxy_server::https_proxy::RouterContext,
) -> Result<()> {
    let Some(location) = ctx.route_container.clone() else {
        return Ok(());
    };
    for (name, value) in &location.plugins {
        if name == "oauth2" {
            use crate::plugins::MiddlewarePlugin;

            if crate::plugins::PLUGINS
                .oauth2
                .response_filter(session, ctx, value)
                .await
                .is_ok_and(|v| v)
            {
                return Ok(());
            }
        }
    }
    Ok(())
}

/// Executes the request plugins
pub async fn execute_request_plugins(
    session: &mut pingora::proxy::Session,
    ctx: &mut crate::proxy_server::https_proxy::RouterContext,
    plugins: &HashMap<String, crate::config::RoutePlugin>,
) -> Result<bool> {
    use crate::plugins::MiddlewarePlugin;
    for (name, value) in plugins {
        match name.as_str() {
            "oauth2" => {
                if crate::plugins::PLUGINS
                    .oauth2
                    .request_filter(session, ctx, value)
                    .await
                    .is_ok_and(|v| v)
                {
                    return Ok(true);
                }
            }
            "request_id" => {
                crate::plugins::PLUGINS
                    .request_id
                    .request_filter(session, ctx, value)
                    .await
                    .ok();
            }
            "basic_auth"
                if crate::plugins::PLUGINS
                    .basic_auth
                    .request_filter(session, ctx, value)
                    .await
                    .is_ok_and(|v| v) =>
            {
                return Ok(true);
            }
            _ => {}
        }
    }
    Ok(false)
}

/// Executes the upstream request plugins
pub async fn execute_upstream_request_plugins(
    session: &mut pingora::proxy::Session,
    upstream_request: &mut pingora::http::RequestHeader,
    ctx: &mut crate::proxy_server::https_proxy::RouterContext,
) -> Result<()> {
    let Some(location) = ctx.route_container.clone() else {
        return Ok(());
    };
    for name in location.plugins.keys() {
        if name == "request_id" {
            crate::plugins::PLUGINS
                .request_id
                .upstream_request_filter(session, upstream_request, ctx)
                .await
                .ok();
        }
    }
    Ok(())
}

/// Executes the upstream response plugins
pub fn execute_upstream_response_plugins(
    session: &mut pingora::proxy::Session,
    upstream_response: &mut pingora::http::ResponseHeader,
    ctx: &mut crate::proxy_server::https_proxy::RouterContext,
) {
    let Some(location) = ctx.route_container.clone() else {
        return;
    };
    for name in location.plugins.keys() {
        if name == "request_id" {
            crate::plugins::PLUGINS
                .request_id
                .upstream_response_filter(session, upstream_response, ctx)
                .ok();
        }
    }
}
