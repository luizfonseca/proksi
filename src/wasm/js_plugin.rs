use wasmer::{imports, Instance, Module, Store};

#[allow(dead_code)]
pub struct MiddlewareFunction {
    pub request_filter: wasmer::Function,
    pub store: Store,
}

pub fn request_filter_templ(request_filter: i32) -> i32 {
    let _ = request_filter;
    return 1;
}

#[allow(dead_code)]
pub fn load_plugin() -> anyhow::Result<MiddlewareFunction> {
    let mut store = Store::default();
    let module = Module::from_binary(&store, include_bytes!("../../assets/middleware-test.wasm"))?;

    // let default_wasm_value = wasmer::Value::I32(1); // >0 is true, 0 is false

    // The module doesn't import anything, so we create an empty import object.
    let import_object = imports! {};
    let instance = Instance::new(&mut store, &module, &import_object)?;

    let request_filter_fn = match instance.exports.get_function("on_request_filter") {
        Ok(value) => value,
        Err(err) => {
            println!("Error loading request_filter: {:?}", err);
            &wasmer::Function::new_typed(&mut store, request_filter_templ)
        }
    };

    Ok(MiddlewareFunction {
        request_filter: request_filter_fn.clone(),
        store,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_plugin() {
        let mut module = load_plugin().unwrap();

        println!(
            "Test: {:?}",
            module
                .request_filter
                .call(&mut module.store, &[wasmer::Value::I32(2),])
        );
    }
}
