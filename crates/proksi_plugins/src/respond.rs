//! Responses of proksi itself (errors, and answers of building blocks such as a `429`), with the
//! location's response blocks applied as to upstreams' responses (e.g. its `headers`), and
//! without the `Server: Pingora` of Pingora's own

use http::header;
use pingora::{http::ResponseHeader, proxy::Session};

use crate::RequestCtx;

/// Writes `response`, with no body
///
/// # Errors
///
/// When the response can't be written to the client.
pub async fn respond(
    session: &mut Session,
    ctx: &mut dyn RequestCtx,
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

/// Writes `response`, then `len` bytes of `body` as they're read (64 KiB at a time). `body` is
/// read on the request's thread: a file, mostly in the page cache.
///
/// # Errors
///
/// When `body` can't be read, ends early, or the response can't be written to the client.
pub async fn respond_streaming(
    session: &mut Session,
    ctx: &mut dyn RequestCtx,
    mut response: ResponseHeader,
    mut body: impl std::io::Read,
    len: u64,
) -> pingora::Result<()> {
    use std::io::Read;

    const CHUNK: u64 = 64 * 1024;
    if len == 0 {
        return respond(session, ctx, response).await;
    }
    response_blocks(session, &mut response, ctx).await;
    session
        .write_response_header(Box::new(response), false)
        .await?;
    let mut left = len;
    while left > 0 {
        let want = left.min(CHUNK);
        // Read into its capacity: no zeroing first
        let mut chunk = Vec::with_capacity(usize::try_from(want).unwrap_or(0));
        let read = (&mut body)
            .take(want)
            .read_to_end(&mut chunk)
            .map_err(|err| {
                pingora::Error::because(pingora::ErrorType::ReadError, "reading the body", err)
            })?;
        if read as u64 != want {
            return Err(pingora::Error::explain(
                pingora::ErrorType::ReadError,
                "the body ended early",
            ));
        }
        left -= want;
        session
            .write_response_body(Some(chunk.into()), left == 0)
            .await?;
    }
    Ok(())
}

/// The response blocks of the location and of the rules the request matched
async fn response_blocks(
    session: &mut Session,
    response: &mut ResponseHeader,
    ctx: &mut dyn RequestCtx,
) {
    ctx.response_blocks(session, response).await;
}

/// Writes an error response (`code`, with no body), unless a response was already written. The
/// connection is closed after it, as Pingora does.
///
/// # Errors
///
/// When the response can't be written to the client.
pub async fn respond_error(
    session: &mut Session,
    ctx: &mut dyn RequestCtx,
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
///
/// # Errors
///
/// When the response can't be written to the client.
pub async fn respond_with_body(
    session: &mut Session,
    ctx: &mut dyn RequestCtx,
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
