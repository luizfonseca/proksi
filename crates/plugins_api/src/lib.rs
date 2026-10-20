#[allow(clippy::wildcard_imports)]
use wit::*;

#[derive(Clone, Debug, PartialEq, Ord, Eq, PartialOrd, Hash)]
pub struct Context {}

#[derive(Clone, Debug, PartialEq, Ord, Eq, PartialOrd, Hash)]
pub struct Session {}
impl Session {
    /// A header of the request: none yet, the host doesn't share them with plugins
    #[must_use]
    pub fn get_header(&self, _key: &str) -> Option<&str> {
        None
    }

    /// The request's header: none yet, the host doesn't share it with plugins
    #[must_use]
    pub fn req_header() -> Option<bool> {
        None
    }
}

pub trait Plugin: Send + Sync {
    fn new_ctx(ctx: String) -> String;

    fn on_request_filter(
        _session: Session,
        _ctx: Context,
    ) -> impl std::future::Future<Output = Result<bool, ()>> {
        async { Ok(true) }
    }
}

// Generated bindings
#[allow(clippy::same_length_and_capacity)]
mod wit {
    wit_bindgen::generate!({
      world: "plugin"
    });
}

wit::export!(Component);

struct Component;

impl wit::Guest for Component {
    fn new_ctx(_ctx: String) -> String {
        String::from("hello")
    }

    fn on_request_filter(_session: &wit::Session, _ctx: String) -> Result<bool, ()> {
        Ok(true)
    }
}
