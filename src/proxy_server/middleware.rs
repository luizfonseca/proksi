use std::collections::HashMap;

use pingora::Result;

use crate::plugins::MiddlewarePlugin;

/// Executes the request and response plugins
pub async fn execute_response_plugins(
    container: &RouteStoreContainer,
    session: &mut pingora_proxy::Session,
    ctx: &mut crate::proxy_server::https_proxy::RouterContext,
) -> Result<()> {
    for (name, value) in &container.plugins {
        match name.as_str() {
            "oauth2" => {
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
            "request_id" => continue,
            _ => {}
        }
    }
    Ok(())
}

/// Executes the request plugins
pub async fn execute_request_plugins(
    session: &mut pingora_proxy::Session,
    ctx: &mut crate::proxy_server::https_proxy::RouterContext,
    plugins: &HashMap<String, crate::config::RoutePlugin>,
) -> Result<bool> {
    for (name, value) in plugins {
        match name.as_str() {
            "oauth2" => {
                use crate::plugins::MiddlewarePlugin;

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
                if crate::plugins::PLUGINS
                    .request_id
                    .request_filter(session, ctx, value)
                    .await
                    .is_ok()
                {
                    // noop, not a blocking plugin
                };
            }
            _ => {}
        }
    }
    Ok(false)
}

/// Executes the upstream request plugins
pub async fn execute_upstream_request_plugins(
    session: &mut pingora_proxy::Session,
    upstream_request: &mut pingora_http::RequestHeader,
    ctx: &mut crate::proxy_server::https_proxy::RouterContext,
) -> Result<()> {
    let container = ctx.route_container.clone().unwrap();

    for name in container.plugins.keys() {
        match name.as_str() {
            "request_id" => {
                crate::plugins::PLUGINS
                    .request_id
                    .upstream_request_filter(session, upstream_request, ctx)
                    .await
                    .ok();
            }
            "other" => continue,
            _ => {}
        }
    }
    Ok(())
}

/// Executes the upstream response plugins
pub fn execute_upstream_response_plugins(
    session: &mut pingora_proxy::Session,
    upstream_response: &mut pingora_http::ResponseHeader,
    ctx: &mut crate::proxy_server::https_proxy::RouterContext,
) {
    let container = ctx.route_container.clone().unwrap();
    for name in container.plugins.keys() {
        match name.as_str() {
            "request_id" => {
                crate::plugins::PLUGINS
                    .request_id
                    .upstream_response_filter(session, upstream_response, ctx)
                    .ok();
            }
            "other" => continue,
            _ => {}
        }
    }
}
