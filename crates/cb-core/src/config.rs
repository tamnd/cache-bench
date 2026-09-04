//! `config.jsonc`, which is where the compiled binaries are.
//!
//! Same keys and same shape as the original's, so a config that works there works here and the other way round.
//! It is JSON with comments because the original's is, and the comments are the only documentation some of those paths have.
//!
//! One placeholder is understood, `${arch}`, which becomes `x86_64` or `aarch64`.
//! Only Dragonfly needs it, because Dragonfly ships a release binary with the architecture in its filename, but it is substituted in every path rather than in that one, since that is what the original does and a rule with an exception in it is a rule somebody will get wrong later.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::cache::CacheKind;

/// The placeholder, spelled the original's way.
const ARCH: &str = "${arch}";

/// Comments and trailing commas, and nothing else.
///
/// The parser's own defaults also take single quoted strings, missing commas, hexadecimal numbers and a leading plus.
/// Those are turned off deliberately. The original strips comments and trailing commas and hands the rest to a strict JSON parser, so a config using any of the rest would work here and fail there, and the whole point of keeping the same file is that it works in both.
const OPTIONS: jsonc_parser::ParseOptions = jsonc_parser::ParseOptions {
    allow_comments: true,
    allow_trailing_commas: true,
    allow_loose_object_property_names: false,
    allow_missing_commas: false,
    allow_single_quoted_strings: false,
    allow_hexadecimal_numbers: false,
    allow_unary_plus_numbers: false,
};

/// Which architecture the binaries were built for.
///
/// The original reads this from the Go runtime and translates `amd64` and `arm64` into the names the Dragonfly release files use.
/// Rust already calls them what Dragonfly calls them, so there is nothing to translate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    /// 64 bit x86.
    X86_64,
    /// 64 bit ARM.
    Aarch64,
}

impl Arch {
    /// The name that goes into a path.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::X86_64 => "x86_64",
            Self::Aarch64 => "aarch64",
        }
    }

    /// The architecture this build of the harness was compiled for.
    ///
    /// `None` on anything else, which is not a failure on its own.
    /// A config that never mentions `${arch}` works fine on a machine we have no name for, and one that does mention it fails when the path is asked for rather than when the file is read.
    #[must_use]
    pub const fn host() -> Option<Self> {
        match std::env::consts::ARCH.as_bytes() {
            b"x86_64" => Some(Self::X86_64),
            b"aarch64" => Some(Self::Aarch64),
            _ => None,
        }
    }
}

/// Anything that stops a config being usable.
#[derive(Debug, thiserror::Error)]
pub enum BadConfig {
    /// The file is not JSON, with or without comments.
    #[error("config is not JSON with comments: {0}")]
    Syntax(String),
    /// The file is JSON but not a `paths` object of strings.
    #[error("config has the wrong shape, expected a paths object of strings: {0}")]
    Shape(#[from] serde_json::Error),
    /// A binary the sweep needs has no path.
    #[error("config has no path for {0:?}, and everything the sweep runs has to be named in it")]
    Missing(String),
    /// A path needs the architecture and this build does not have a name for the one it is running on.
    #[error("the path for {name:?} is {path:?}, and this build does not have a name for {arch}")]
    UnknownArch {
        /// The key that was asked for.
        name: String,
        /// The path as written, placeholder and all.
        path: String,
        /// What the target calls itself.
        arch: &'static str,
    },
}

/// Where every binary the sweep runs lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    paths: BTreeMap<String, PathBuf>,
}

/// The file as written, before the placeholder is dealt with.
#[derive(Debug, Deserialize)]
struct Raw {
    paths: BTreeMap<String, String>,
}

impl Config {
    /// Read a `config.jsonc`.
    ///
    /// Comments and trailing commas are stripped first, the same way the original strips them.
    /// `arch` is normally [`Arch::host`], and passing `None` leaves `${arch}` in place to be complained about later, by which time we know which binary somebody actually wanted.
    ///
    /// # Errors
    ///
    /// If the text is not JSON with comments, or is not a `paths` object of strings.
    pub fn parse(text: &str, arch: Option<Arch>) -> Result<Self, BadConfig> {
        let value: serde_json::Value = jsonc_parser::parse_to_serde_value(text, &OPTIONS)
            .map_err(|e| BadConfig::Syntax(e.to_string()))?;
        let raw: Raw = serde_json::from_value(value)?;
        let paths = raw
            .paths
            .into_iter()
            .map(|(name, path)| {
                let path = match arch {
                    Some(arch) => path.replace(ARCH, arch.name()),
                    None => path,
                };
                (name, PathBuf::from(path))
            })
            .collect();
        Ok(Self { paths })
    }

    /// The load generator.
    ///
    /// # Errors
    ///
    /// If the config does not name it.
    pub fn memtier(&self) -> Result<&Path, BadConfig> {
        self.path("memtier")
    }

    /// One cache server's binary.
    ///
    /// # Errors
    ///
    /// If the config does not name it, or names it with an `${arch}` this build cannot fill in.
    pub fn binary(&self, kind: CacheKind) -> Result<&Path, BadConfig> {
        self.path(kind.name())
    }

    /// Every key the file carries, in sorted order.
    ///
    /// The original reads whatever is there and takes an empty string for anything missing, so a typo in a key produces a run that fails at exec time rather than at read time.
    /// This is here so `doctor` can say which keys it found instead.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.paths.keys().map(String::as_str)
    }

    /// A path by name.
    fn path(&self, name: &str) -> Result<&Path, BadConfig> {
        let path = self
            .paths
            .get(name)
            .ok_or_else(|| BadConfig::Missing(name.to_owned()))?;
        if path.to_string_lossy().contains(ARCH) {
            return Err(BadConfig::UnknownArch {
                name: name.to_owned(),
                path: path.to_string_lossy().into_owned(),
                arch: std::env::consts::ARCH,
            });
        }
        Ok(path)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{Arch, Config};
    use crate::cache::CacheKind;

    // The config this repository ships. If a server is added to CacheKind and not to the file, this is what says so.
    const OURS: &str = include_str!("../../../config.jsonc");

    #[test]
    fn our_own_config_names_every_server_and_the_load_generator() {
        let cfg = Config::parse(OURS, Some(Arch::X86_64)).unwrap();
        assert!(cfg.memtier().is_ok());
        for kind in CacheKind::ALL {
            assert!(cfg.binary(kind).is_ok(), "no path for {kind}");
        }
    }

    // Nothing in the file is there by accident, so a key that is not a server and is not memtier is a typo.
    #[test]
    fn our_own_config_has_nothing_else_in_it() {
        let cfg = Config::parse(OURS, Some(Arch::X86_64)).unwrap();
        for name in cfg.names() {
            assert!(
                name == "memtier" || name.parse::<CacheKind>().is_ok(),
                "{name} is not a cache server or the load generator"
            );
        }
    }

    // The original's own config, verbatim, which has six servers and no yo.
    #[test]
    fn the_originals_config_reads() {
        const THEIRS: &str = r#"{
    // Paths to all compiled binaries.
    "paths": {
        "memtier": "memtier_benchmark",
        "valkey": "../valkey/src/valkey-server",
        "redis": "../redis/src/redis-server",
        "dragonfly": "../dragonfly/dragonfly-${arch}",
        "memcache": "../memcached/memcached",
        "pogocache": "../pogocache/pogocache",
        // It is expected that the dotnet GarnetServer is already compiled
        // for Release.
        "garnet": "../garnet/main/GarnetServer/bin/Release/net9.0/GarnetServer"
    }
}"#;
        let cfg = Config::parse(THEIRS, Some(Arch::Aarch64)).unwrap();
        assert_eq!(
            cfg.binary(CacheKind::Valkey).unwrap(),
            std::path::Path::new("../valkey/src/valkey-server")
        );
        // The one server it does not have, and the error says which.
        let missing = cfg.binary(CacheKind::Yo).unwrap_err().to_string();
        assert!(missing.contains("yo"), "{missing}");
    }

    // Dragonfly ships a release binary with the architecture in its filename, and it is the only reason the placeholder exists.
    #[test]
    fn the_arch_placeholder_is_filled_in() {
        for (arch, want) in [
            (Arch::X86_64, "../dragonfly/dragonfly-x86_64"),
            (Arch::Aarch64, "../dragonfly/dragonfly-aarch64"),
        ] {
            let cfg = Config::parse(OURS, Some(arch)).unwrap();
            assert_eq!(
                cfg.binary(CacheKind::Dragonfly).unwrap(),
                std::path::Path::new(want)
            );
        }
    }

    // A machine we have no name for is only a problem for the path that needs the name, and only when somebody asks for it.
    #[test]
    fn an_unknown_arch_fails_late_and_says_so() {
        let cfg = Config::parse(OURS, None).unwrap();
        assert!(cfg.binary(CacheKind::Valkey).is_ok());
        let err = cfg.binary(CacheKind::Dragonfly).unwrap_err().to_string();
        assert!(err.contains("dragonfly"), "{err}");
        assert!(err.contains("${arch}"), "{err}");
    }

    // Comments are the only documentation some of those paths have, and a trailing comma is what you leave behind when you delete the last entry.
    #[test]
    fn comments_and_a_trailing_comma_are_fine() {
        let text = r#"{
    // A line comment.
    /* And a block one. */
    "paths": {
        "memtier": "memtier_benchmark",
        "redis": "../redis/src/redis-server",
    }
}"#;
        let cfg = Config::parse(text, Some(Arch::X86_64)).unwrap();
        assert_eq!(cfg.names().collect::<Vec<_>>(), ["memtier", "redis"]);
    }

    #[test]
    fn rubbish_is_refused_with_a_reason() {
        assert!(Config::parse("not json at all", None).is_err());
        assert!(Config::parse("{}", None).is_err());
        assert!(Config::parse(r#"{"paths": {"redis": 7}}"#, None).is_err());
    }

    // Not an assertion about the host, just that the two names we know are the two names Dragonfly uses.
    #[test]
    fn the_host_arch_is_one_we_have_a_name_for_or_it_is_none() {
        match Arch::host() {
            Some(arch) => assert!(["x86_64", "aarch64"].contains(&arch.name())),
            None => assert!(!["x86_64", "aarch64"].contains(&std::env::consts::ARCH)),
        }
    }
}
