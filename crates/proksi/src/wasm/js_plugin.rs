use wasmtime::{
    component::{Linker, Resource},
    Engine, Store,
};
use wasmtime_wasi::{add_to_linker_async, preview1::WasiP1Ctx, WasiCtxBuilder, WasiView};

// wasmtime::component::bindgen!({
//   path: "../plugins_api/wit/plugin.wit",
//   with: {
//     "session": SessionTest,
//   }
// });

#[allow(dead_code)]
#[derive(Clone)]
pub struct SessionTest {}

impl SessionTest {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {}
    }
    #[allow(dead_code)]
    pub fn get_header(&self, _key: String) -> String {
        String::from("test")
    }
}

#[allow(dead_code)]
pub async fn load_plugin(path: &[u8]) -> anyhow::Result<()> {
    let mut config = wasmtime::Config::new();
    config.wasm_reference_types(true);
    config.debug_info(false);
    config.async_support(true);

    let engine = Engine::new(&config)?;
    let wasi_ctx = WasiCtxBuilder::new()
        .inherit_stdio()
        .inherit_stdout()
        .build_p1();

    let mut linker: Linker<WasiP1Ctx> = wasmtime::component::Linker::new(&engine);

    let session_now = SessionTest::new();
    // let mut abc = wasmtime_wasi::ResourceTable::new();
    // let res = wasi_ctx.table().push(session_now.clone())?;

    add_to_linker_async(&mut linker)?;
    let mut store = Store::new(&engine, wasi_ctx);
    let resource = store.data_mut().table().push(session_now.clone())?;
    let resource_id = resource.rep();
    let module = wasmtime::component::Component::from_binary(&engine, path)?;

    // instance

    let ty = wasmtime::component::ResourceType::host::<SessionTest>();
    linker.root().resource("session", ty, |mut storex, rep| {
        storex
            .data_mut()
            .table()
            .delete::<SessionTest>(Resource::new_own(rep));
        Ok(())
    })?;

    // let res_before_move: Resource<SessionTest> =
    //     wasmtime::component::Resource::new_own(resource.rep());

    // let into_any = res.try_into_resource_any(&mut store)?;

    // linker
    //     .root()
    //     .func_wrap("[method]session.get-header", |mut st, (input,)| {
    //         let result = 1;

    //         Ok(())
    //     });
    // linker.root().func_new(
    //     "[method]session.get-header",
    //     move |mut storex, params, results| {
    //         // in resources, the first param is the resource (self)
    //         // let input = match params[1].clone() {
    //         //     wasmtime::component::Val::String(v) => v,
    //         //     _ => panic!("invalid input"),
    //         // };
    //         // let ss = storex.data_mut().table().get(&resource)?;
    //         // let v = ss.get_header(input);
    //         results[0] = wasmtime::component::Val::Option(Some(Box::new(
    //             wasmtime::component::Val::String("123".into()),
    //         )));

    //         // storex.data_mut().table().delete(resource)?;

    //         Ok(())
    //     },
    // )?;

    let instance = linker.instantiate_async(&mut store, &module).await?;
    let req_filter_fn = instance
        .get_typed_func::<(Resource<SessionTest>, String), (Result<bool, ()>,)>(
            &mut store,
            "on-request-filter",
        )
        .unwrap();

    let resource = store.data_mut().table().push(session_now.clone())?;

    let ret = req_filter_fn
        .call_async(
            &mut store,
            (Resource::new_own(resource.rep()), String::from("hello")),
        )
        .await?;

    req_filter_fn.post_return(store)?;

    println!("ret: {:?}", ret);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_load_plugin() {
        load_plugin(include_bytes!(
            "../../../../target/wasm32-wasip2/debug/plugin_request_id.wasm"
        ))
        .await
        .unwrap();

        assert_eq!(1, 1)
    }
}
