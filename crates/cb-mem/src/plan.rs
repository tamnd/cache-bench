//! How many keys go in, and what they weigh.
//!
//! A bytes-per-entry figure is a division, and both sides of it have to be known rather than estimated. The denominator is the number of distinct keys the server was left holding; the numerator is the resident set that holding them cost. Getting the denominator right is the whole difficulty, because memtier reports operations rather than keys and a SET pass that writes the same key twice leaves one entry behind.
//!
//! So the pass is arranged to make the two the same number. With `--key-pattern P:P` memtier splits the key range evenly across its clients and each one walks its own slice in order, so a pass of exactly one operation per key writes every key once and no key twice. That needs the key range to divide by the client count, which is a thing to refuse rather than round, since rounding it means the last slice runs off the end of the range and the count that goes in the file is not the count that went in the server.
//!
//! None of this asks the server how many keys it has. `DBSIZE` and `stats curr_items` are two different questions with two different answers about two different things, and an engine-specific denominator is exactly the kind of special handling the fairness rules exist to prevent.

use cb_core::Profile;

/// A SET-only pass sized to leave a known number of entries behind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    /// How many distinct keys the server will be holding when the pass finishes.
    pub entries: u64,
    /// Operations per client, which is memtier's `-n`.
    pub per_client: u64,
    /// How many clients there are, which is connections times bench threads.
    pub clients: u64,
}

impl Plan {
    /// Size a pass against a profile.
    ///
    /// # Errors
    ///
    /// If the entry count does not divide evenly by the number of clients the profile asks for, because a pass that does not cover the key range exactly leaves a number of entries nobody knows.
    pub fn new(profile: &Profile, entries: u64) -> Result<Self, BadPlan> {
        let clients = u64::from(profile.connections_per_thread) * u64::from(profile.bench_threads);
        if clients == 0 {
            return Err(BadPlan::NoClients);
        }
        if entries == 0 {
            return Err(BadPlan::NoEntries);
        }
        if !entries.is_multiple_of(clients) {
            return Err(BadPlan::Uneven { entries, clients });
        }
        Ok(Self {
            entries,
            per_client: entries / clients,
            clients,
        })
    }

    /// What the keys and the values themselves weigh, in bytes.
    ///
    /// The part of the resident set that is the data rather than the machinery around it. Subtracting it from the peak is what turns a total into an overhead, and the two are different claims: at a hundred-odd bytes of payload per key, no index can halve a total whatever it does to an overhead.
    ///
    /// The key half is exact. memtier is given an empty `--key-prefix`, so a key is the decimal spelling of its number and the total is a digit count over the range. The value half is an average, because memtier draws each value's size uniformly from the profile's range, and an average over tens of millions of draws is not where the error in this measurement is.
    #[must_use]
    pub fn payload(&self, profile: &Profile) -> u64 {
        digits(self.entries)
            .saturating_add(self.entries.saturating_mul(profile.size_range.average()))
    }
}

/// How many characters it takes to write every number from one to `n`.
///
/// Counted a decade at a time rather than one number at a time, because the entry counts here run to tens of millions and the answer is wanted before the pass rather than after it.
fn digits(n: u64) -> u64 {
    let mut total = 0u64;
    let mut width = 1u64;
    let mut low = 1u64;
    while low <= n {
        // The last number with this many digits, or the end of the range, whichever comes first.
        let high = low.saturating_mul(10).saturating_sub(1).min(n);
        total = total.saturating_add((high - low + 1).saturating_mul(width));
        let Some(next) = low.checked_mul(10) else {
            break;
        };
        low = next;
        width += 1;
    }
    total
}

/// An entry count that cannot be measured against this profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BadPlan {
    /// The profile asks for no clients at all.
    #[error("this profile runs no clients, so there is nothing to write keys with")]
    NoClients,
    /// No keys asked for.
    #[error("a measurement of nought entries divides into a bytes-per-entry figure of infinity")]
    NoEntries,
    /// The key range does not split evenly.
    #[error(
        "{entries} entries do not divide by the {clients} clients this profile runs, so the pass would not cover the key range exactly and the number of keys left behind would be a guess. Pick a multiple of {clients}."
    )]
    Uneven {
        /// What was asked for.
        entries: u64,
        /// What it has to divide by.
        clients: u64,
    },
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use cb_core::Profiles;

    use super::{BadPlan, Plan, digits};

    fn profile() -> cb_core::Profile {
        let text = std::fs::read_to_string("../../profiles.toml").unwrap();
        let profiles = Profiles::parse(&text).unwrap();
        profiles.get("wsl32").unwrap().clone()
    }

    #[test]
    fn one_operation_per_key_and_the_clients_split_it_evenly() {
        let p = profile();
        let clients = u64::from(p.connections_per_thread) * u64::from(p.bench_threads);
        let plan = Plan::new(&p, clients * 1000).unwrap();
        assert_eq!(plan.per_client * plan.clients, plan.entries);
        assert_eq!(plan.per_client, 1000);
    }

    // A pass that does not cover the key range exactly leaves a number of entries nobody knows, and that number is the denominator of everything this crate reports.
    #[test]
    fn a_count_that_does_not_divide_is_refused_and_says_what_to_pick() {
        let p = profile();
        let clients = u64::from(p.connections_per_thread) * u64::from(p.bench_threads);
        let err = Plan::new(&p, clients * 1000 + 1).unwrap_err();
        assert!(matches!(err, BadPlan::Uneven { .. }));
        assert!(err.to_string().contains(&clients.to_string()), "{err}");
    }

    #[test]
    fn nought_entries_is_refused_rather_than_divided_by() {
        assert_eq!(Plan::new(&profile(), 0), Err(BadPlan::NoEntries));
    }

    // Counted by hand: nine one digit numbers, ninety two digit ones, nine hundred of three.
    #[test]
    fn the_keys_are_counted_a_decade_at_a_time() {
        assert_eq!(digits(0), 0);
        assert_eq!(digits(9), 9);
        assert_eq!(digits(10), 9 + 2);
        assert_eq!(digits(99), 9 + 90 * 2);
        assert_eq!(digits(100), 9 + 90 * 2 + 3);
        assert_eq!(digits(999), 9 + 90 * 2 + 900 * 3);
    }

    // The slow way, for a range small enough to walk.
    #[test]
    fn the_decade_count_agrees_with_counting_them_one_at_a_time() {
        let slow: u64 = (1..=5000u64).map(|n| n.to_string().len() as u64).sum();
        assert_eq!(digits(5000), slow);
    }

    #[test]
    fn the_payload_is_the_keys_plus_the_values() {
        let p = profile();
        let clients = u64::from(p.connections_per_thread) * u64::from(p.bench_threads);
        let plan = Plan::new(&p, clients).unwrap();
        let want = digits(plan.entries) + plan.entries * p.size_range.average();
        assert_eq!(plan.payload(&p), want);
        // And it is not just the values, which is the mistake that would make a key of any length free.
        assert!(plan.payload(&p) > plan.entries * p.size_range.average());
    }
}
