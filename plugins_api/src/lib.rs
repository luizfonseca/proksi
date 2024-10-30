use wit::*;

#[derive(Clone, Debug, PartialEq, Ord, Eq, PartialOrd, Hash)]
pub struct Context {}

#[derive(Clone, Debug, PartialEq, Ord, Eq, PartialOrd, Hash)]
pub struct Session {}
impl Session {
    pub fn get_header(&self, _key: &str) -> Option<&str> {
        unimplemented!()
    }

    pub fn req_header() -> Option<bool> {
        unimplemented!()
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
