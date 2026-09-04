//! `hosts.toml`, which is where sweeps run.
//!
//! Absent by default, and absent means run here. That is the normal case: you ssh to the box, start tmux, and run the sweep on it.
//!
//! The file is gitignored and `hosts.example.toml` is what is committed in its place. Machine names and addresses do not go in a public repository, and the entries here name an ssh config entry rather than an address so that there is one fewer place for one to end up.
//! Nothing under `results/` ever carries a hostname either. A host is identified in the generated documents by what it is, from `host.json`, and never by what it is called.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::profile::Profiles;

/// One machine a sweep can run on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Host {
    /// A name from your ssh config, or absent for this machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh: Option<String>,
    /// Which hardware profile this machine is.
    pub profile: String,
    /// Where the cache servers and the load generator are built on it.
    ///
    /// Relative paths in `config.jsonc` resolve against this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workdir: Option<PathBuf>,
}

impl Host {
    /// Whether this entry means this machine.
    #[must_use]
    pub const fn is_local(&self) -> bool {
        self.ssh.is_none()
    }
}

/// Every machine the file names.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Hosts {
    /// By name, which is what `--host` takes.
    #[serde(default)]
    pub hosts: BTreeMap<String, Host>,
}

impl Hosts {
    /// Read a `hosts.toml`.
    ///
    /// # Errors
    ///
    /// If the file is not TOML of the right shape.
    pub fn parse(text: &str) -> Result<Self, BadHosts> {
        toml::from_str(text).map_err(|e| BadHosts::Shape(e.to_string()))
    }

    /// One host by name.
    ///
    /// # Errors
    ///
    /// If the file does not name it.
    pub fn get(&self, name: &str) -> Result<&Host, BadHosts> {
        self.hosts.get(name).ok_or_else(|| BadHosts::NoSuchHost {
            name: name.to_owned(),
            known: self.hosts.keys().cloned().collect::<Vec<_>>().join(", "),
        })
    }

    /// Whether every host names a profile that exists.
    ///
    /// Worth checking at read time rather than at run time, since the alternative is finding out two weeks into a sweep that the standby host was never going to start.
    ///
    /// # Errors
    ///
    /// If a host names a profile the profiles file does not have.
    pub fn check_against(&self, profiles: &Profiles) -> Result<(), BadHosts> {
        for (name, host) in &self.hosts {
            if profiles.get(&host.profile).is_err() {
                return Err(BadHosts::NoSuchProfile {
                    host: name.clone(),
                    profile: host.profile.clone(),
                });
            }
        }
        Ok(())
    }
}

/// Anything that stops a hosts file being usable.
#[derive(Debug, thiserror::Error)]
pub enum BadHosts {
    /// The file is not TOML of the right shape.
    #[error("hosts are not readable: {0}")]
    Shape(String),
    /// No host by that name.
    #[error("no host called {name:?}, the file has {known}")]
    NoSuchHost {
        /// What was asked for.
        name: String,
        /// What is there.
        known: String,
    },
    /// A host names a profile that does not exist.
    #[error("host {host} says it is profile {profile:?}, which is not in profiles.toml")]
    NoSuchProfile {
        /// The host.
        host: String,
        /// The profile it claims.
        profile: String,
    },
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::Hosts;
    use crate::profile::Profiles;

    // The committed template, which is the only hosts file this repository will ever have.
    const EXAMPLE: &str = include_str!("../../../hosts.example.toml");

    #[test]
    fn the_committed_template_reads() {
        let hosts = Hosts::parse(EXAMPLE).unwrap();
        assert_eq!(hosts.hosts.len(), 3);
        assert!(hosts.get("local").unwrap().is_local());
        assert!(!hosts.get("big").unwrap().is_local());
    }

    // Every profile the template names has to exist, or the template is teaching people to write a file that does not work.
    #[test]
    fn the_template_names_profiles_we_ship() {
        let profiles = Profiles::parse(include_str!("../../../profiles.toml")).unwrap();
        Hosts::parse(EXAMPLE)
            .unwrap()
            .check_against(&profiles)
            .unwrap();
    }

    // The template is committed and a real one is not, so nothing in it may look like a real machine.
    #[test]
    fn the_template_has_no_real_addresses_in_it() {
        for host in Hosts::parse(EXAMPLE).unwrap().hosts.values() {
            let Some(ssh) = &host.ssh else { continue };
            assert!(
                !ssh.contains('.') && !ssh.contains('@'),
                "{ssh} looks like an address rather than an ssh config entry"
            );
        }
    }

    // Absent means run here, which is the normal case and has to be the case that needs no file at all.
    #[test]
    fn no_file_means_this_machine() {
        assert!(Hosts::default().hosts.is_empty());
        assert!(Hosts::parse("").unwrap().hosts.is_empty());
    }

    #[test]
    fn a_profile_that_does_not_exist_is_caught_at_read_time() {
        let profiles = Profiles::parse(include_str!("../../../profiles.toml")).unwrap();
        let hosts = Hosts::parse("[hosts.somewhere]\nprofile = \"nothing\"\n").unwrap();
        let err = hosts.check_against(&profiles).unwrap_err().to_string();
        assert!(err.contains("somewhere"), "{err}");
        assert!(err.contains("nothing"), "{err}");
    }
}
