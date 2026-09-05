//! The bar colours.
//!
//! Five of the six are matplotlib's default cycle, which is where the original got them, and the pink is hand picked.
//!
//! # Why this is not the original's rule
//!
//! The original assigns a colour by position in the list of cache servers a results file turns out to hold, and that list is the sorted set of names. So a results file with a server missing shifts every colour after it, and so does a server added.
//!
//! That rule survived `yo` by luck: `yo` sorts after all six of the original's names, so a seventh colour on the end left the first six where they were. `rugo` sorts between `redis` and `valkey`, and under the original's rule adding it would have silently recoloured Valkey, Pogocache and yo in every chart this project has ever drawn.
//!
//! So the colour is a property of the server here, from the table below, and not of where it landed. The original's six keep the original's six colours permanently, which is the thing the positional rule was for and the thing it could not actually promise. This is D22 in `divergences.md`.

use cb_core::CacheKind;

/// A colour for each cache server, by position.
///
/// Kept as the list it always was, because it is what `--check` compares a redrawn chart against and what a reader comparing two projects' charts is looking at. The order is the order the original assigns them in, which is sorted name order over the original's six.
pub const COLORS: [&str; 8] = [
    "#ff7f0e", "#d62728", "#1f77b4", "#e64098", "#8c564b", "#2ca02c", "#9467bd", "#17becf",
];

/// Which colour belongs to which server.
///
/// The first six rows are the original's assignment written out: sorted name order over its six names, against `COLORS` in order. Reading it off the sorted order is what this table exists to stop, so it is written out rather than computed.
///
/// The two after them are ours. Purple for `yo` was already published and does not move. Cyan for `rugo` is the next unused colour in the same matplotlib cycle, so it does not look bolted on.
const ASSIGNED: [(CacheKind, &str); 8] = [
    (CacheKind::Dragonfly, COLORS[0]),
    (CacheKind::Garnet, COLORS[1]),
    (CacheKind::Memcache, COLORS[2]),
    (CacheKind::Pogocache, COLORS[3]),
    (CacheKind::Redis, COLORS[4]),
    (CacheKind::Valkey, COLORS[5]),
    (CacheKind::Yo, COLORS[6]),
    (CacheKind::Rugo, COLORS[7]),
];

/// The colour this cache server is drawn in.
///
/// # Errors
///
/// If the name is not a server this build knows. The original indexes a table by position and panics past the end of it, which is a fine way to find out at the end of a two week sweep that an engine has no colour, but not a fine way to be told.
pub fn color(cache: &str) -> Result<&'static str, NoColor> {
    ASSIGNED
        .iter()
        .find(|(kind, _)| kind.name() == cache)
        .map(|(_, hex)| *hex)
        .ok_or_else(|| NoColor(cache.to_owned()))
}

/// A colour, as three channels from zero to one.
///
/// The same range matplotlib works in, because the outline colour is worked out by multiplying and the multiplication has to happen on the same numbers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgb(pub [f64; 3]);

impl Rgb {
    /// Read a six digit hex colour.
    ///
    /// # Errors
    ///
    /// If it is not a hash followed by six hex digits, which is the only form anything in this project uses.
    pub fn parse(hex: &str) -> Result<Self, BadColor> {
        let digits = hex.strip_prefix('#').filter(|d| d.len() == 6);
        let channel = |at: usize| {
            digits
                .and_then(|d| d.get(at..at + 2))
                .and_then(|p| u8::from_str_radix(p, 16).ok())
                .map(|v| f64::from(v) / 255.0)
        };
        match (channel(0), channel(2), channel(4)) {
            (Some(r), Some(g), Some(b)) => Ok(Self([r, g, b])),
            _ => Err(BadColor(hex.to_owned())),
        }
    }

    /// The colour a bar is outlined in, which is the bar's own colour darkened.
    ///
    /// The original writes it as each channel times 0.4 clamped into range, and the clamp never does anything because darkening cannot take a channel out of range. It is kept because it is what the original computes.
    #[must_use]
    pub fn edge(self) -> Self {
        Self(
            self.0
                .map(|c| (c * crate::axis::EDGE_SCALE).clamp(0.0, 1.0)),
        )
    }
}

/// Text that is not a six digit hex colour.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is not a colour, expected a hash and six hex digits")]
pub struct BadColor(pub String);

/// A cache server in a results file that this build has no colour for.
///
/// Which means this build has never heard of it, since every server it knows is in [`ASSIGNED`] and a test says so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoColor(pub String);

impl std::fmt::Display for NoColor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the results name a cache server called {:?}, which this build has no colour for",
            self.0
        )
    }
}

impl std::error::Error for NoColor {}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use cb_core::CacheKind;

    use super::{ASSIGNED, COLORS, Rgb, color};

    // The six the original uses, in its order, because a chart drawn here and a chart drawn there should be the same chart.
    #[test]
    fn the_first_six_are_the_originals() {
        assert_eq!(
            &COLORS[..6],
            [
                "#ff7f0e", "#d62728", "#1f77b4", "#e64098", "#8c564b", "#2ca02c"
            ]
        );
    }

    // The whole point of the table. Under the original's positional rule these six are the sorted order of its six names, and they have to keep the colours they had whatever is added afterwards.
    #[test]
    fn the_originals_six_keep_the_colours_they_had() {
        let want = [
            ("dragonfly", "#ff7f0e"),
            ("garnet", "#d62728"),
            ("memcache", "#1f77b4"),
            ("pogocache", "#e64098"),
            ("redis", "#8c564b"),
            ("valkey", "#2ca02c"),
        ];
        for (cache, hex) in want {
            assert_eq!(color(cache).unwrap(), hex, "{cache}");
        }
    }

    // Adding rugo under the original's rule would have recoloured these three, because `rugo` sorts between `redis` and `valkey`.
    // This is the test that says it did not.
    #[test]
    fn an_engine_added_in_the_middle_of_the_alphabet_moves_nothing() {
        assert_eq!(color("valkey").unwrap(), "#2ca02c");
        assert_eq!(color("pogocache").unwrap(), "#e64098");
        assert_eq!(color("yo").unwrap(), "#9467bd");
    }

    // A server this build knows and has no colour for would draw a chart with two bars the same colour, which is a chart that misleads rather than one that fails.
    #[test]
    fn every_engine_has_a_colour_and_no_two_share_one() {
        for kind in CacheKind::ALL {
            assert!(color(kind.name()).is_ok(), "{kind} has no colour");
        }
        assert_eq!(ASSIGNED.len(), CacheKind::ALL.len());
        let mut hexes: Vec<&str> = ASSIGNED.iter().map(|(_, hex)| *hex).collect();
        hexes.sort_unstable();
        hexes.dedup();
        assert_eq!(hexes.len(), ASSIGNED.len());
    }

    #[test]
    fn every_colour_is_distinct() {
        let mut seen: Vec<&str> = COLORS.to_vec();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), COLORS.len());
    }

    // The number the original's chart records show for the orange, worked out the way it works it out.
    #[test]
    fn an_outline_is_the_bar_darkened() {
        let edge = Rgb::parse("#ff7f0e").unwrap().edge();
        assert_eq!(
            edge,
            Rgb([0.4, 0.199_215_686_274_509_8, 0.021_960_784_313_725_49])
        );
    }

    #[test]
    fn every_colour_in_the_table_reads_back() {
        for hex in COLORS {
            assert!(Rgb::parse(hex).is_ok(), "{hex} does not parse");
        }
        assert!(Rgb::parse("ff7f0e").is_err());
        assert!(Rgb::parse("#ff7f0").is_err());
        assert!(Rgb::parse("#ff7f0g").is_err());
    }

    // The inverse of what this test used to say. It used to be that an eighth engine was an error, because there were seven colours; now an engine is known or it is not, and the ninth engine is a row in the table rather than a limit.
    #[test]
    fn a_cache_server_this_build_does_not_know_is_an_error_rather_than_a_panic() {
        assert!(color("rugo").is_ok());
        assert!(color("keydb").is_err());
        assert_eq!(
            color("keydb").unwrap_err().to_string(),
            "the results name a cache server called \"keydb\", which this build has no colour for"
        );
    }
}
