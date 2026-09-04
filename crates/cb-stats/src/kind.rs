//! Which of the four aggregates a chosen file is.

use std::fmt;
use std::str::FromStr;

/// One of the four files a cell reduces to.
///
/// The order is the order the original calls them in, and that order is not cosmetic: its run count is a global that shrinks with each call, so `median` sees all 31 runs and `average` sees 17 of them.
/// Corrected mode makes the order irrelevant, and keeps it anyway so that the two modes can be read side by side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// The run in the middle, and the only one anything plots.
    Median,
    /// The fastest run that survived the trim.
    Best,
    /// The slowest run that survived the trim.
    Worst,
    /// The mean over the runs that survived the trim.
    Average,
}

impl Kind {
    /// All four, in the order the original writes them.
    pub const ALL: [Self; 4] = [Self::Median, Self::Best, Self::Worst, Self::Average];

    /// The name in the filename and in `info.kind`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Median => "median",
            Self::Best => "best",
            Self::Worst => "worst",
            Self::Average => "average",
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Text that is not one of the four.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is not an aggregate, expected median, best, worst or average")]
pub struct BadKind(pub String);

impl FromStr for Kind {
    type Err = BadKind;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|k| k.name() == s)
            .ok_or_else(|| BadKind(s.to_owned()))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::Kind;

    #[test]
    fn every_name_round_trips() {
        for kind in Kind::ALL {
            assert_eq!(kind.name().parse::<Kind>().unwrap(), kind);
        }
        assert!("mean".parse::<Kind>().is_err());
    }

    // The order is the original's call order, and in upstream mode it decides how many runs each aggregate sees.
    #[test]
    fn the_order_is_the_originals() {
        assert_eq!(
            Kind::ALL.map(Kind::name),
            ["median", "best", "worst", "average"]
        );
    }
}
