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
    response_blocks(session, &mut response, ctx).await;
    session.write_response_header(Box::new(response), true).await?;
    // Pingora sends HTTP/2 headers without ending the stream, and ends it only for sessions it
    // finishes: one dropped after an error (an upstream that can't be reached) would be reset,
    // and clients would see a failure instead of this response
    if session.is_http2() {
        session.downstream_session.finish_body().await?;
    }
    Ok(())
}

/// The response blocks of the location and of the rules the request matched
async fn response_blocks(
    session: &mut Session,
    response: &mut ResponseHeader,
    ctx: &mut RouterContext,
) {
    if let Some(location) = ctx.route_container.clone() {
        location.blocks.response(session, response, ctx).await;
    }
    crate::rules::response(session, response, ctx).await;
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

/// Writes `response` with `body` (its `Content-Length` is set)
pub async fn respond_with_body(
    session: &mut Session,
    ctx: &mut RouterContext,
    mut response: ResponseHeader,
    body: bytes::Bytes,
) -> pingora::Result<()> {
    response_blocks(session, &mut response, ctx).await;
    response.insert_header(header::CONTENT_LENGTH, body.len())?;
    let end = body.is_empty();
    session.write_response_header(Box::new(response), end).await?;
    if !end {
        session.write_response_body(Some(body), true).await?;
    }
    Ok(())
}
