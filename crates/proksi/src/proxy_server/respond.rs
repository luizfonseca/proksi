//! Responses of proksi itself (errors, and answers of building blocks such as a `429`), with the
//! location's response blocks applied as to upstreams' responses (e.g. its `headers`), and
//! without the `Server: Pingora` of Pingora's own

use http::header;
use pingora::{http::ResponseHeader, proxy::Session};

use super::https_proxy::RouterContext;

/// Writes `response`, with no body
pub async fn respond(
    session: &mut Session,
    ctx: &mut RouterContext,
    mut response: ResponseHeader,
) -> pingora::Result<()> {
    if let Some(location) = ctx.route_container.clone() {
        location.blocks.response(session, &mut response, ctx).await;
    }
    session.write_response_header(Box::new(response), true).await
}

/// Writes an error response (`code`, with no body), unless a response was already written. The
/// connection is closed after it, as Pingora does.
pub async fn respond_error(
    session: &mut Session,
    ctx: &mut RouterContext,
    code: u16,
) -> pingora::Result<()> {
    session.set_keepalive(None);
    if session
        .response_written()
        .is_some_and(|written| !written.status.is_informational() || written.status == 101)
    {
        return Ok(());
    }
    respond(session, ctx, error_response(code)?).await
}

/// An error response, as Pingora's without its `Server` header
fn error_response(code: u16) -> pingora::Result<ResponseHeader> {
    let mut response = ResponseHeader::build(code, Some(2))?;
    response.insert_header(header::CONTENT_LENGTH, "0")?;
    response.insert_header(header::CACHE_CONTROL, "private, no-store")?;
    Ok(response)
}
