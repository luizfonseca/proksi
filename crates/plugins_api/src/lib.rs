#[allow(clippy::wildcard_imports)]
use wit::*;

pub use wit::Session;

pub trait Plugin: Send + Sync {
    fn new() -> Self
    where
        Self: Sized;
    fn new_ctx(&self, ctx: String) -> String;

    fn on_request_filter(&self, _session: &Session, _ctx: String) -> Result<bool, ()> {
        Ok(true)
    }
}

mod wit {
    wit_bindgen::generate!({
      world: "plugin",
      skip: ["init-plugin"]
    });
}

wit::export!(Component);

/// Registers the provided type as a Proksi plugin.
#[macro_export]
macro_rules! register_plugin {
    ($plugin_type:ty) => {
        #[export_name = "init-plugin"]
        pub extern "C" fn __init_plugin() {
            std::env::set_current_dir(std::env::var("PWD").unwrap()).unwrap();
            plugins_api::register_plugin(|| Box::new(<$plugin_type as Plugin>::new()));
        }
    };
}

#[doc(hidden)]
pub fn register_plugin(build_plugin: fn() -> Box<dyn Plugin>) {
    unsafe { PLUGIN = Some((build_plugin)()) }
}

fn plugin() -> &'static mut dyn Plugin {
    unsafe { PLUGIN.as_deref_mut().unwrap() }
}

static mut PLUGIN: Option<Box<dyn Plugin>> = None;

struct Component;

impl wit::Guest for Component {
    fn new_ctx(_ctx: String) -> String {
        String::from("hello from wit")
    }

    fn on_request_filter(session: &wit::Session, ctx: String) -> Result<bool, ()> {
        plugin().on_request_filter(session, ctx)
    }
}
