use cookie::{time::OffsetDateTime, Cookie, SameSite};

use crate::plugins::jwt;

use super::{UserFromProvider, COOKIE_NAME};

pub(super) fn create_secure_cookie<'a>(
    user: UserFromProvider,
    jwt_secret: String,
    host: &'a String,
) -> Result<Cookie<'a>, anyhow::Error> {
    let jwt_token = jwt::encode_jwt(&user.email, &jwt_secret.as_bytes())?;

    Ok(Cookie::build((COOKIE_NAME, jwt_token))
        .secure(true)
        .domain(host)
        .path("/")
        .expires(OffsetDateTime::now_utc().checked_add(cookie::time::Duration::days(1)))
        .http_only(true)
        .same_site(SameSite::Lax)
        .build())
}
