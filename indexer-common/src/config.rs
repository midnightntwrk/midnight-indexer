// This file is part of midnight-indexer.
// Copyright (C) Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use figment::{
    Figment,
    providers::{Env, Format, Yaml},
};
use serde::Deserialize;
use std::{env, fs};

const CONFIG_FILE: &str = "CONFIG_FILE";

/// Env var suffix: `APP__X_FILE=/path` is read and materialised into `APP__X`.
const FILE_SUFFIX: &str = "_FILE";

/// Rejects oversized files (secrets are a few KB at most).
const MAX_SECRET_FILE_SIZE: u64 = 64 * 1024;

/// Extension methods for "configuration structs" which can be deserialized.
pub trait ConfigExt
where
    Self: for<'de> Deserialize<'de>,
{
    /// Load the configuration from the file at the value of the `CONFIG_FILE` environment variable
    /// or `config.yaml` by default, with an overlay provided by environment variables prefixed with
    /// `"APP__"` and split/nested via `"__"`.
    ///
    /// `APP__X_FILE` env vars are materialised into `APP__X` before Figment runs, so secrets can
    /// be mounted as Kubernetes Secret files. Directly-set env vars take precedence.
    fn load() -> Result<Self, Box<figment::Error>> {
        materialise_file_env_vars();

        let config_file = env::var(CONFIG_FILE)
            .map(Yaml::file_exact)
            .unwrap_or(Yaml::file_exact("config.yaml"));

        let config = Figment::new()
            .merge(config_file)
            .merge(Env::prefixed("APP__").split("__"))
            .extract()?;

        Ok(config)
    }
}

fn materialise_file_env_vars() {
    let file_vars = env::vars()
        .filter(|(key, _)| key.starts_with("APP__") && key.ends_with(FILE_SUFFIX))
        .collect::<Vec<_>>();

    for (file_key, path) in file_vars {
        let target_key = &file_key[..file_key.len() - FILE_SUFFIX.len()];
        if env::var(target_key).is_ok() {
            continue;
        }
        match read_secret_file(&path) {
            Ok(value) => {
                // SAFETY: Single-threaded at load time (called before any tokio runtime starts).
                unsafe {
                    env::set_var(target_key, value);
                }
            }
            Err(error) => {
                // stderr because this runs before logger init.
                eprintln!("warning: secret file {file_key}='{path}' skipped: {error}");
            }
        }
    }
}

fn read_secret_file(path: &str) -> Result<String, String> {
    let metadata = fs::metadata(path).map_err(|error| format!("cannot stat '{path}': {error}"))?;

    if !metadata.is_file() {
        return Err(format!("'{path}' is not a regular file"));
    }

    if metadata.len() > MAX_SECRET_FILE_SIZE {
        return Err(format!(
            "'{path}' is {} bytes, exceeding the {MAX_SECRET_FILE_SIZE}-byte limit",
            metadata.len()
        ));
    }

    fs::read_to_string(path)
        .map(|content| content.trim().to_owned())
        .map_err(|error| format!("cannot read '{path}': {error}"))
}

impl<T> ConfigExt for T where T: for<'de> Deserialize<'de> {}

#[cfg(test)]
mod tests {
    use crate::config::{CONFIG_FILE, ConfigExt, MAX_SECRET_FILE_SIZE, materialise_file_env_vars};
    use assert_matches::assert_matches;
    use serde::Deserialize;
    use std::{env, io::Write};

    #[test]
    fn test_load() {
        unsafe {
            env::set_var("APP__API__PORT", "4242");
        }

        let config = MainConfig::load();
        assert_matches!(
            config,
            Ok(MainConfig { config: Config { api: api::Config { port, .. } } }) if port == 4242
        );

        unsafe {
            env::set_var(CONFIG_FILE, "nonexistent.yaml");
        }
        let config = Config::load();
        assert!(config.is_err());
    }

    #[test]
    fn test_materialise_file_env_vars_reads_file() {
        let mut file = tempfile::NamedTempFile::new().expect("tempfile");
        writeln!(file, "secret-from-file").expect("write tempfile");
        let path = file.path().display().to_string();

        unsafe {
            env::remove_var("APP__TEST_SECRET");
            env::set_var("APP__TEST_SECRET_FILE", &path);
        }

        materialise_file_env_vars();

        assert_eq!(
            env::var("APP__TEST_SECRET").expect("APP__TEST_SECRET should be set"),
            "secret-from-file"
        );

        unsafe {
            env::remove_var("APP__TEST_SECRET");
            env::remove_var("APP__TEST_SECRET_FILE");
        }
    }

    #[test]
    fn test_materialise_file_env_vars_does_not_overwrite() {
        let mut file = tempfile::NamedTempFile::new().expect("tempfile");
        writeln!(file, "from-file").expect("write tempfile");
        let path = file.path().display().to_string();

        unsafe {
            env::set_var("APP__OVERRIDE_SECRET", "direct-value");
            env::set_var("APP__OVERRIDE_SECRET_FILE", &path);
        }

        materialise_file_env_vars();

        assert_eq!(
            env::var("APP__OVERRIDE_SECRET").expect("APP__OVERRIDE_SECRET should be set"),
            "direct-value",
            "directly-set env var must win over *_FILE"
        );

        unsafe {
            env::remove_var("APP__OVERRIDE_SECRET");
            env::remove_var("APP__OVERRIDE_SECRET_FILE");
        }
    }

    #[test]
    fn test_materialise_file_env_vars_missing_file_is_not_fatal() {
        unsafe {
            env::remove_var("APP__MISSING_SECRET");
            env::set_var("APP__MISSING_SECRET_FILE", "/this/file/does/not/exist");
        }

        materialise_file_env_vars();

        assert!(
            env::var("APP__MISSING_SECRET").is_err(),
            "unreadable file should not set the target env var",
        );

        unsafe {
            env::remove_var("APP__MISSING_SECRET_FILE");
        }
    }

    #[test]
    fn test_materialise_file_env_vars_trims_whitespace() {
        let mut file = tempfile::NamedTempFile::new().expect("tempfile");
        write!(file, "  \tpadded-secret\n  \n").expect("write tempfile");
        let path = file.path().display().to_string();

        unsafe {
            env::remove_var("APP__TRIM_SECRET");
            env::set_var("APP__TRIM_SECRET_FILE", &path);
        }

        materialise_file_env_vars();

        assert_eq!(
            env::var("APP__TRIM_SECRET").expect("APP__TRIM_SECRET should be set"),
            "padded-secret"
        );

        unsafe {
            env::remove_var("APP__TRIM_SECRET");
            env::remove_var("APP__TRIM_SECRET_FILE");
        }
    }

    #[test]
    fn test_materialise_file_env_vars_rejects_oversized_file() {
        let mut file = tempfile::NamedTempFile::new().expect("tempfile");
        let payload = vec![b'x'; MAX_SECRET_FILE_SIZE as usize + 1];
        file.write_all(&payload).expect("write tempfile");
        let path = file.path().display().to_string();

        unsafe {
            env::remove_var("APP__OVERSIZED_SECRET");
            env::set_var("APP__OVERSIZED_SECRET_FILE", &path);
        }

        materialise_file_env_vars();

        assert!(
            env::var("APP__OVERSIZED_SECRET").is_err(),
            "oversized file should not set the target env var",
        );

        unsafe {
            env::remove_var("APP__OVERSIZED_SECRET_FILE");
        }
    }

    #[derive(Debug, Clone, Deserialize)]
    pub struct MainConfig {
        /// Application sepcific configuration.
        #[serde(flatten)]
        pub config: Config,
    }

    /// Application sepcific configuration.
    #[derive(Debug, Clone, Deserialize)]
    pub struct Config {
        pub api: api::Config,
    }

    mod api {
        use serde::Deserialize;

        #[derive(Debug, Clone, Deserialize)]
        pub struct Config {
            pub port: u16,
        }
    }
}
