//! `proksi check`: loads and checks the configuration as Proksi would when starting, without
//! starting anything, for editors and CI

use std::fmt;

use clap::Parser;

use super::{load_from_path, Command, Config};

/// Why a configuration can't be used
#[derive(Debug, Clone, PartialEq)]
pub enum CheckError {
    /// Proksi wouldn't start with it, and why
    Invalid(String),
}

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CheckError::Invalid(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for CheckError {}

/// Loads `path` (a file, or a directory with `proksi.hcl` or `proksi.yaml`) as Proksi's
/// configuration, and sums it up
pub fn check_file(path: &str) -> Result<String, CheckError> {
    let config = load_from_path(path, &Config::parse_from(["proksi"]), false)
        .map_err(|err| CheckError::Invalid(err.to_string()))?;
    let mut summary = vec![count(config.hosts().len(), "host")];
    if !config.tcp.is_empty() {
        summary.push(count(config.tcp.len(), "tcp block"));
    }
    Ok(summary.join(", "))
}

fn count(number: usize, what: &str) -> String {
    if number == 1 {
        format!("1 {what}")
    } else {
        format!("{number} {what}s")
    }
}

/// `proksi check [-c <path>]`: checks the configuration and exits (1 when it's invalid); returns
/// when not asked to check
pub fn exit_if_requested(fallback: &str) {
    let Ok(cli) = Config::try_parse() else {
        return;
    };
    let Some(Command::Check { config_path }) = cli.command else {
        return;
    };
    let path = config_path
        .or(cli.config_path.map(|path| path.to_string()))
        .unwrap_or_else(|| fallback.to_string());
    match check_file(&path) {
        Ok(summary) => {
            println!("{path}: valid ({summary})");
            std::process::exit(0);
        }
        Err(err) => {
            eprintln!("{path}: {err}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::check_file;

    fn file(text: &str) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("proksi.hcl");
        std::fs::write(&path, text).unwrap();
        (dir, path.to_str().unwrap().to_string())
    }

    #[test]
    fn test_a_valid_configuration_is_summed_up() {
        let (_dir, path) = file(
            "host \"a.test\" {\n  location \"/\" {\n    upstream \"app:3000\" {}\n  }\n}\nhost \"b.test\" {\n  location \"/\" {\n    upstream \"app:3000\" {}\n  }\n}\n",
        );
        assert_eq!(check_file(&path), Ok("2 hosts".to_string()));
    }

    #[test]
    fn test_an_invalid_configuration_says_why() {
        let (_dir, path) = file("host \"a.test\" {\n  locaton \"/\" {}\n}\n");
        let err = check_file(&path).unwrap_err().to_string();
        assert!(err.contains("locaton"), "{err}");
    }
}
