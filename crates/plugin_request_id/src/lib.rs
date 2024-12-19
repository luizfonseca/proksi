use plugins_api::{register_plugin, Plugin};

pub struct RequestIdPlugin {}

impl Plugin for RequestIdPlugin {
    fn new() -> Self {
        Self {}
    }
    fn new_ctx(&self, _: String) -> String {
        String::from("Adds request Id to every request")
    }

    fn on_request_filter(&self, session: &plugins_api::Session, _ctx: String) -> Result<bool, ()> {
        let v = session.get_header("test");
        println!("test: {:?}", v);
        Ok(true)
    }
}

register_plugin!(RequestIdPlugin);
