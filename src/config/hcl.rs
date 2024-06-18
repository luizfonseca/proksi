use std::io::Read;

use hcl::{
    eval::{Context, FuncArgs},
    Value,
};

pub struct HclFormat;
impl figment::providers::Format for HclFormat {
    type Error = hcl::Error;

    const NAME: &'static str = "HCL";

    fn from_str<'de, T: serde::de::DeserializeOwned>(string: &'de str) -> Result<T, Self::Error> {
        hcl::eval::from_str(string, &get_hcl_context())
    }
}

/// Function to read a file from a given path. Useful for reading configuration files.
/// Note that this function is not a part of the HCL specification, but a custom function.
/// Example:
/// ```hcl
/// // HCL document
/// config = read_file("config.toml")
/// ```
fn read_hcl_file(args: FuncArgs) -> Result<Value, String> {
    let path = args[0].as_str().unwrap();
    if !path.ends_with(".hcl") {
        return Err(format!("File must be a HCL file: {}", path));
    }

    let Ok(mut file) = std::fs::File::open(path) else {
        return Err(format!("file not found: {}", path));
    };

    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();

    Ok(Value::Object(hcl::from_str(&contents).unwrap()))
}

/// Function to retrieve an environment variable from a given HCL template. Useful for secrets.
/// Note that this function is not a part of the HCL specification, but a custom function.
/// Note that the environment variable name must be defined or an error will be returned.
/// Example:
/// ```hcl
/// // HCL document
/// secret = env("JWT_SECRET")
/// host = env("HOST")
/// port = env("PORT")
/// ```
fn get_env_var(args: FuncArgs) -> Result<Value, String> {
    let key = args[0].as_str().unwrap();
    let Ok(var) = std::env::var(key) else {
        return Err(format!("Environment variable {} not found", key));
    };

    Ok(Value::String(var))
}

/// Get the HCL context for figment
fn get_hcl_context<'a>() -> Context<'a> {
    let env_func = hcl::eval::FuncDef::builder()
        .param(hcl::eval::ParamType::String)
        .build(get_env_var);

    let read_file_func = hcl::eval::FuncDef::builder()
        .param(hcl::eval::ParamType::String)
        .build(read_hcl_file);

    let mut context = hcl::eval::Context::new();
    context.declare_func("env", env_func);
    context.declare_func("import", read_file_func);

    context
}

/// HashiCorp Configuration Language (HCL) provider for figment
pub type Hcl = figment::providers::Data<HclFormat>;
