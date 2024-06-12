use std::collections::HashMap;

use pingora::Result;

use crate::{plugins::MiddlewarePlugin, stores::routes::RouteStoreContainer};

/// Executes the request and response plugins
pub async fn execute_response_plugins(
    container: &RouteStoreContainer,
    session: &mut pingora::proxy::Session,
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
            "basic_auth" => {
                if crate::plugins::PLUGINS
                    .basic_auth
                    .request_filter(session, ctx, value)
                    .await
                    .is_ok_and(|v| v)
                {
                    return Ok(true);
                }
            }
            _ => {}
        }
    }
    Ok(false)
}

/// Executes the upstream request plugins
pub async fn execute_upstream_request_plugins(
    container: &crate::stores::routes::RouteStoreContainer,
    session: &mut pingora::proxy::Session,
    upstream_request: &mut pingora::http::RequestHeader,
    ctx: &mut crate::proxy_server::https_proxy::RouterContext,
) -> Result<()> {
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
    container: &crate::stores::routes::RouteStoreContainer,
    session: &mut pingora::proxy::Session,
    upstream_response: &mut pingora::http::ResponseHeader,
    ctx: &mut crate::proxy_server::https_proxy::RouterContext,
) {
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
