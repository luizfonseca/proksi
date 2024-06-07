use std::collections::HashMap;

use pingora::Result;

pub async fn execute_response_plugins(
    session: &mut pingora_proxy::Session,
    ctx: &crate::proxy_server::https_proxy::RouterContext,
    plugins: &HashMap<String, crate::config::RoutePlugin>,
) -> Result<()> {
    for (name, value) in plugins {
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

pub async fn execute_request_plugins(
    session: &mut pingora_proxy::Session,
    ctx: &mut crate::proxy_server::https_proxy::RouterContext,
    plugins: &HashMap<String, crate::config::RoutePlugin>,
) -> Result<()> {
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
                    return Ok(());
                }
            }
            "request_id" => continue,
            _ => {}
        }
    }
    Ok(())
}
