use std::{fmt::Write, io::Read, path::Path};

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
        let mut body = hcl::parse(string)?;
        check_duplicate_blocks(&body, "")?;
        crate::rules::unlabel_rules(&mut body);
        hcl::from_body(hcl::eval::Evaluate::evaluate(&body, &get_hcl_context())?)
    }
}

/// Rejects what HCL would silently merge or drop:
/// - blocks repeated with the same labels (e.g. two `location "/api"` in a host),
///   which would be read as a list
/// - an attribute and blocks with the same name (e.g. `host = {...}` and `host "a" {}`),
///   where the blocks would replace the attribute
fn check_duplicate_blocks(body: &hcl::Body, parent: &str) -> Result<(), hcl::Error> {
    let attributes = body
        .attributes()
        .map(hcl::Attribute::key)
        .collect::<std::collections::HashSet<_>>();
    let mut seen = std::collections::HashSet::new();
    for block in body.blocks() {
        if attributes.contains(block.identifier()) {
            return Err(hcl::Error::Message(format!(
                "`{}` is used both as an attribute and as a block, use one or the other",
                block.identifier()
            )));
        }

        let mut name = block.identifier().to_string();
        for label in block.labels() {
            let _ = write!(name, " \"{}\"", label.as_str());
        }

        let repeatable = matches!(block.identifier(), "header" | "query" | "cookie");
        if !block.labels().is_empty() && !repeatable && !seen.insert(name.clone()) {
            let parent = if parent.is_empty() {
                String::new()
            } else {
                format!(" in `{parent}`")
            };
            return Err(hcl::Error::Message(format!(
                "duplicate block `{name}`{parent}"
            )));
        }

        check_duplicate_blocks(block.body(), &name)?;
    }
    Ok(())
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
/// config = import("config.toml")
/// ```
#[allow(clippy::needless_pass_by_value)]
fn read_hcl_file(args: FuncArgs) -> Result<Value, String> {
    let Some(path) = args.first().and_then(Value::as_str) else {
        return Err("import() expects a file path".to_string());
    };
    let path = Path::new(path);

    if !path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("hcl"))
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
    if let Err(err) = file.read_to_string(&mut contents) {
        return Err(format!(
            "failed to read file {}: {err}",
            path.to_string_lossy()
        ));
    }

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
    let Some(key) = args.first().and_then(Value::as_str) else {
        return Err("env() expects a variable name".to_string());
    };
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

#[cfg(test)]
mod tests {
    use figment::providers::Format;

    use super::HclFormat;

    #[test]
    fn test_duplicate_labelled_blocks_are_rejected() {
        let err = HclFormat::from_str::<hcl::Value>(
            r#"
host "a.test" {
  location "/api" {
  }
  location "/api" {
  }
}
"#,
        )
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "duplicate block `location \"/api\"` in `host \"a.test\"`"
        );
    }

    #[test]
    fn test_duplicate_top_level_blocks_are_rejected() {
        let err = HclFormat::from_str::<hcl::Value>("host \"a.test\" {}\nhost \"a.test\" {}\n")
            .unwrap_err();

        assert_eq!(err.to_string(), "duplicate block `host \"a.test\"`");
    }

    #[test]
    fn test_attribute_and_blocks_with_the_same_name_are_rejected() {
        let err = HclFormat::from_str::<hcl::Value>(
            "host = {\n  \"a.test\" = {}\n}\nhost \"b.test\" {}\n",
        )
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "`host` is used both as an attribute and as a block, use one or the other"
        );
    }

    #[test]
    fn test_blocks_with_different_labels_are_accepted() {
        let value = HclFormat::from_str::<hcl::Value>(
            r#"
host "a.test" {
  location "/api" {
  }
  location "/*" {
  }
}
host "b.test" {
  location "/api" {
  }
}
"#,
        );

        assert!(value.is_ok(), "{value:?}");
    }
}
