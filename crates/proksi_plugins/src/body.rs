//! Reading a request's whole body, for blocks that check or change it. The body is read before
//! anything is sent upstream, into one buffer, and kept for the next blocks: what a block sets
//! with [`RequestCtx::set_request_body`] is what's sent upstream, with its own `Content-Length`.

use bytes::{Bytes, BytesMut};
use http::header;
use pingora::proxy::Session;

use crate::RequestCtx;

/// Why a body couldn't be read
#[derive(Debug, PartialEq, Eq)]
pub enum Unread {
    /// Over `max` bytes
    TooLarge,
    /// The client stopped sending it, or sent it too slowly
    Failed,
}

/// The request's body, up to `max` bytes: the one a previous block read, or else the client's,
/// read now. A larger body is refused before it's read when `Content-Length` says so.
///
/// # Errors
///
/// [`Unread::TooLarge`] over `max` bytes, [`Unread::Failed`] when it can't be read.
pub async fn read(
    session: &mut Session,
    ctx: &mut dyn RequestCtx,
    max: u64,
) -> Result<Bytes, Unread> {
    if let Some(body) = ctx.request_body() {
        return if body.len() as u64 > max {
            Err(Unread::TooLarge)
        } else {
            Ok(body.clone())
        };
    }
    let declared = session
        .req_header()
        .headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok()?.parse::<u64>().ok());
    if declared.is_some_and(|declared| declared > max) {
        return Err(Unread::TooLarge);
    }

    // One allocation when the client says how long it is
    let capacity = usize::try_from(declared.unwrap_or(0).min(max)).unwrap_or(0);
    let mut body = BytesMut::with_capacity(capacity);
    while let Some(part) = session
        .read_request_body()
        .await
        .map_err(|_| Unread::Failed)?
    {
        if (body.len() + part.len()) as u64 > max {
            return Err(Unread::TooLarge);
        }
        body.extend_from_slice(&part);
    }
    let body = body.freeze();
    ctx.set_request_body(body.clone());
    Ok(body)
}
