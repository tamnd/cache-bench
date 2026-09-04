//! The bar colours.
//!
//! Five of the six are matplotlib's default cycle, which is where the original got them, and the pink is hand picked. They are assigned by position in the list of cache servers a results file turns out to hold, not by which server it is, so a results file with a server missing shifts every colour after it.
//!
//! That is the original's rule and it is kept, because a chart drawn here has to be comparable with a chart drawn there. It works out because the list is the sorted set of names, `yo` sorts last, and appending a seventh colour therefore leaves the original's six assignments exactly where they were.

/// A colour for each cache server, by position.
///
/// The first six are the original's. The purple is ours, for `yo`, and it comes from the same matplotlib cycle as the rest so that it does not look bolted on.
pub const COLORS: [&str; 7] = [
    "#ff7f0e", "#d62728", "#1f77b4", "#e64098", "#8c564b", "#2ca02c", "#9467bd",
];

/// The colour for the cache server at this position in the list.
///
/// # Errors
///
/// If there are more cache servers than colours. The original indexes the table directly and panics, which is a fine way to find out at the end of a two week sweep that the eighth engine has no colour, but not a fine way to be told.
pub fn color(at: usize) -> Result<&'static str, TooManyCaches> {
    COLORS.get(at).copied().ok_or(TooManyCaches { at })
}

/// More cache servers in a results file than there are colours to draw them with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooManyCaches {
    /// The position that had no colour, so the count is this plus one.
    pub at: usize,
}

impl std::fmt::Display for TooManyCaches {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} cache servers in the results but only {} colours to draw them with",
            self.at + 1,
            COLORS.len()
        )
    }
}

impl std::error::Error for TooManyCaches {}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{COLORS, color};

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

    #[test]
    fn every_colour_is_distinct() {
        let mut seen: Vec<&str> = COLORS.to_vec();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), COLORS.len());
    }

    #[test]
    fn an_eighth_cache_server_is_an_error_rather_than_a_panic() {
        assert!(color(6).is_ok());
        assert!(color(7).is_err());
        assert_eq!(
            color(7).unwrap_err().to_string(),
            "8 cache servers in the results but only 7 colours to draw them with"
        );
    }
}
