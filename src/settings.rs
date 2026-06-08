use std::collections::VecDeque;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::time::Duration;

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
use serde::{Deserialize, Serialize};
use snafu::{prelude::*, Location};
use xdg::BaseDirectories;

use crate::daemon::ShikaneArgs;
use crate::error;
use crate::profile::Profile;

#[derive(Clone, Debug)]
pub struct Settings {
    pub profiles: VecDeque<Profile>,
    pub skip_tests: bool,
    pub oneshot: bool,
    pub timeout: Duration,
    pub config_path: PathBuf,
}

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct SettingsToml {
    pub timeout: Option<u64>,
    #[serde(default, rename = "profile")]
    pub profiles: VecDeque<Profile>,
}

impl Settings {
    pub fn from_args(args: ShikaneArgs) -> Self {
        let (config, path) = match parse_settings_toml(args.config) {
            Ok(config) => config,
            Err(err) => {
                error!("{}", error::report(err.as_ref()));
                std::process::exit(1);
            }
        };

        let timeout = config.timeout.unwrap_or(args.timeout);

        Self {
            profiles: config.profiles,
            skip_tests: args.skip_tests,
            oneshot: args.oneshot,
            timeout: Duration::from_millis(timeout),
            config_path: path,
        }
    }

    pub fn reload_config(&mut self, config: Option<PathBuf>) -> Result<(), Box<dyn snafu::Error>> {
        let config = config.unwrap_or(self.config_path.clone());
        debug!("reloading config from {:?}", std::fs::canonicalize(&config));
        let (config, path) = parse_settings_toml(Some(config))?;
        self.profiles = config.profiles;
        self.config_path = path;
        Ok(())
    }
}

fn parse_settings_toml(
    config_path: Option<PathBuf>,
) -> Result<(SettingsToml, PathBuf), Box<dyn snafu::Error>> {
    let config_path = match config_path {
        None => {
            let xdg_dirs = BaseDirectories::with_prefix("shikane").context(BaseDirectoriesCtx)?;
            std::fs::create_dir_all(xdg_dirs.get_config_home()).context(ConfigPathCtx)?;
            xdg_dirs
                .place_config_file("config.toml")
                .context(ConfigPathCtx)?
        }
        Some(path) => path,
    };
    ensure_config_file_exists(&config_path).context(ReadConfigFileCtx)?;
    let s = std::fs::read_to_string(&config_path).context(ReadConfigFileCtx)?;
    let mut config: SettingsToml = toml::from_str(&s).context(TomlDeserializeCtx)?;
    config
        .profiles
        .iter_mut()
        .enumerate()
        .for_each(|(idx, p)| p.index = idx);
    Ok((config, config_path))
}

fn ensure_config_file_exists(config_path: &PathBuf) -> Result<(), std::io::Error> {
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(config_path)
    {
        Ok(_) => Ok(()),
        Err(err) if err.kind() == ErrorKind::AlreadyExists => Ok(()),
        Err(err) => Err(err),
    }
}

#[derive(Debug, Snafu)]
#[snafu(context(suffix(Ctx)))]
pub enum SettingsError {
    #[snafu(display("[{location}] Problem with XDG directories"))]
    BaseDirectories {
        source: xdg::BaseDirectoriesError,
        location: Location,
    },
    #[snafu(display("[{location}] Cannot read config file"))]
    ReadConfigFile {
        source: std::io::Error,
        location: Location,
    },
    #[snafu(display("[{location}] Cannot place config file in XDG config directory"))]
    ConfigPath {
        source: std::io::Error,
        location: Location,
    },
    #[snafu(display("[{location}] Cannot deserialize settings from TOML"))]
    TomlDeserialize {
        source: toml::de::Error,
        location: Location,
    },
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    use super::parse_settings_toml;

    fn test_config_path(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "shikane-{name}-{}-{}.toml",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let _ = fs::remove_file(&path);
        path
    }

    #[test]
    fn reads_read_only_config() {
        let path = test_config_path("readonly");
        fs::write(&path, "").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();

        let result = parse_settings_toml(Some(path.clone()));

        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        fs::remove_file(&path).unwrap();
        result.unwrap();
    }

    #[test]
    fn creates_missing_config() {
        let path = test_config_path("missing");

        parse_settings_toml(Some(path.clone())).unwrap();

        assert!(path.exists());
        fs::remove_file(&path).unwrap();
    }
}
