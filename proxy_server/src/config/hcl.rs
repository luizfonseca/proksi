use std::{io::Read, path::Path};

use hcl::{
    eval::{Context, FuncArgs},
    Value,
};

#[allow(clippy::module_name_repetitions)]
pub struct HclFormat;
impl figment::providers::Format for HclFormat {
    type Error = hcl::Error;

    const NAME: &'static str = "HCL";

    fn from_str<T: serde::de::DeserializeOwned>(string: &str) -> Result<T, Self::Error> {
        hcl::eval::from_str(string, &get_hcl_context())
    }
}

/// Function to retrieve the number of CPUs available on the system.
/// Useful for setting the number of worker threads.
/// Note that this function is not a part of the HCL specification, but a custom function.
/// Example:
/// ```hcl
/// // HCL document
/// worker_threads = num_cpus()
/// ```
#[allow(clippy::needless_pass_by_value, clippy::unnecessary_wraps)]
fn num_cpus(_: FuncArgs) -> Result<Value, String> {
    let num_cpus = num_cpus::get();
    Ok(Value::Number(num_cpus.into()))
}

/// Function to read a file from a given path. Useful for reading configuration files.
/// Note that this function is not a part of the HCL specification, but a custom function.
/// Example:
/// ```hcl
/// // HCL document
/// config = read_file("config.toml")
/// ```
#[allow(clippy::needless_pass_by_value)]
fn read_hcl_file(args: FuncArgs) -> Result<Value, String> {
    let path = Path::new(args[0].as_str().unwrap());

    if !path
        .extension()
        .map_or(false, |ext| ext.eq_ignore_ascii_case("hcl"))
    {
        return Err(format!(
            "File must be a HCL file: {}",
            path.to_string_lossy()
        ));
    }
    // convert ath to absolute path
    let Ok(path) = std::path::absolute(path) else {
        return Err(format!("file not found: {}", path.to_string_lossy()));
    };

    let Ok(mut file) = std::fs::File::open(&path) else {
        return Err(format!("file not found: {}", path.to_string_lossy()));
    };

    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();

    let value = match hcl::eval::from_str::<Value>(&contents, &get_hcl_context()) {
        Ok(v) => v,
        Err(e) => {
            return Err(format!("failed to parse hcl file: {e}"));
        }
    };

    Ok(value)
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
#[allow(clippy::needless_pass_by_value)]
fn get_env_var(args: FuncArgs) -> Result<Value, String> {
    let key = args[0].as_str().unwrap();
    let Ok(var) = std::env::var(key) else {
        return Err(format!("Environment variable {key} not found"));
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

    let num_cpus_func = hcl::eval::FuncDef::builder().build(num_cpus);

    let mut context = hcl::eval::Context::new();
    context.declare_func("env", env_func);
    context.declare_func("import", read_file_func);
    context.declare_func("num_cpus", num_cpus_func);

    context
}

/// hashicorp Configuration Language (HCL) provider for figment
pub type Hcl = figment::providers::Data<HclFormat>;
