//! The two fixed decimal number types the on disk format uses.
//!
//! The original writes every throughput and latency number with exactly three decimal places, and every perf counter with none, by formatting the float itself and splicing the text into the JSON.
//! No serialiser setting produces that, so these newtypes format themselves and hand the resulting text to `serde_json` as a raw value.
//!
//! Doing it this way also removes float round trip drift.
//! The file is the source of truth, and reading one back gives the same decimal string it went out with.

use std::fmt;

use serde::de::{self, Deserializer, Visitor};
use serde::ser::{Error as _, Serializer};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

/// A number written with exactly three decimal places, such as `104.475` or `0.000`.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub struct Fixed3(pub f64);

/// A number written with no decimal places, such as `642245372237`.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub struct Fixed0(pub f64);

impl fmt::Display for Fixed3 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.3}", self.0)
    }
}

impl fmt::Display for Fixed0 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.0}", self.0)
    }
}

/// Turn already formatted decimal text into a JSON number rather than a JSON string.
///
/// The only way this fails is a value with no decimal form at all, meaning an infinity or a NaN.
/// That is a bug upstream of here rather than something to paper over with a zero, so it is an error.
fn emit<S: Serializer>(text: String, s: S) -> Result<S::Ok, S::Error> {
    let raw = RawValue::from_string(text).map_err(S::Error::custom)?;
    raw.serialize(s)
}

impl Serialize for Fixed3 {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        emit(self.to_string(), s)
    }
}

impl Serialize for Fixed0 {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        emit(self.to_string(), s)
    }
}

impl<'de> Deserialize<'de> for Fixed3 {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        f64::deserialize(d).map(Self)
    }
}

impl<'de> Deserialize<'de> for Fixed0 {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        f64::deserialize(d).map(Self)
    }
}

/// One perf counter, in whichever shape it appeared in the file.
///
/// A run file holds these as JSON strings, because the original copies the text out of `perf stat` output without looking at it, and some of that text is `<not supported>` on a machine with no hardware counter for the event.
/// A chosen file holds the same counters as JSON numbers, because the selection step reparses them as floats on the way through.
///
/// Both shapes have to round trip byte for byte, so this keeps whichever one it was given and converts only on request.
///
/// `PLACES` is how many decimal places the number form is written with, and it is a property of which counter this is rather than of its value.
/// The original writes `cpu_utilized` with three and every event count with none, so it is [`CpuCounter`] and [`EventCounter`] rather than one type that guesses from the value.
#[derive(Debug, Clone, PartialEq)]
pub enum Counter<const PLACES: usize> {
    /// The run file form, which is whatever `perf stat` printed, verbatim.
    Text(String),
    /// The chosen file form, which is a number.
    Number(f64),
}

/// `cpu_utilized`, which is a ratio and is written with three decimal places.
pub type CpuCounter = Counter<3>;

/// An event count, which is written with none.
pub type EventCounter = Counter<0>;

impl<const PLACES: usize> Counter<PLACES> {
    /// The counter as a float.
    ///
    /// Text that is not a number reads as zero.
    /// That is not a guess, it is what the original does, because it pulls these through a JSON library whose float accessor returns zero for a string it cannot parse.
    /// That is how a `<not supported>` branch counter becomes a `0` in the chosen file and a zero height bar on a chart, and reproducing it is the point.
    #[must_use]
    pub fn as_f64(&self) -> f64 {
        match self {
            Self::Text(t) => t.parse().unwrap_or(0.0),
            Self::Number(n) => *n,
        }
    }

    /// Whether this counter carries a real measurement.
    ///
    /// False for `<not supported>` and anything else the machine could not count.
    /// A chart that plots an unsupported counter as zero is making a claim about the engine that the hardware never made.
    #[must_use]
    pub fn is_measured(&self) -> bool {
        match self {
            Self::Text(t) => t.parse::<f64>().is_ok(),
            Self::Number(_) => true,
        }
    }
}

impl<const PLACES: usize> Serialize for Counter<PLACES> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Text(t) => s.serialize_str(t),
            Self::Number(n) => emit(format!("{n:.PLACES$}"), s),
        }
    }
}

impl<'de, const PLACES: usize> Deserialize<'de> for Counter<PLACES> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V<const PLACES: usize>;

        impl<const PLACES: usize> Visitor<'_> for V<PLACES> {
            type Value = Counter<PLACES>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a perf counter, as a string or a number")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(Counter::Text(v.to_owned()))
            }

            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
                Ok(Counter::Number(v))
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(Counter::Number(widen_u64(v)))
            }

            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(Counter::Number(widen_i64(v)))
            }
        }

        d.deserialize_any(V::<PLACES>)
    }
}

/// Cycle counts run to about twelve digits, which is well inside what a double holds exactly, so this is lossless for anything perf will ever report.
/// It is a named function rather than an inline cast so that the two places in this crate that could lose precision are somewhere a reader can find.
#[allow(clippy::cast_precision_loss)]
fn widen_u64(v: u64) -> f64 {
    v as f64
}

#[allow(clippy::cast_precision_loss)]
fn widen_i64(v: i64) -> f64 {
    v as f64
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{CpuCounter, EventCounter, Fixed0, Fixed3};

    #[test]
    fn three_places_always() {
        assert_eq!(Fixed3(104.475).to_string(), "104.475");
        assert_eq!(Fixed3(1.5).to_string(), "1.500");
        assert_eq!(Fixed3(0.0).to_string(), "0.000");
        assert_eq!(Fixed3(199_445.96).to_string(), "199445.960");
    }

    #[test]
    fn no_places_at_all() {
        assert_eq!(Fixed0(642_245_372_237.0).to_string(), "642245372237");
        assert_eq!(Fixed0(0.0).to_string(), "0");
    }

    // Go and Rust both round a decimal tie to the even digit.
    // The perf averages are integer sums divided by a run count, so they land on a tie often enough for it to matter, and if a future toolchain changed this the difference would show up as a handful of cycles counts off by one with nothing to explain it.
    #[test]
    fn ties_go_to_even() {
        assert_eq!(Fixed0(2.5).to_string(), "2");
        assert_eq!(Fixed0(3.5).to_string(), "4");
        assert_eq!(Fixed0(-2.5).to_string(), "-2");
    }

    #[test]
    fn fixed_serialises_as_a_number_not_a_string() {
        assert_eq!(serde_json::to_string(&Fixed3(1.5)).unwrap(), "1.500");
        assert_eq!(serde_json::to_string(&Fixed0(7.0)).unwrap(), "7");
    }

    #[test]
    fn a_counter_keeps_the_shape_it_arrived_in() {
        let text: EventCounter = serde_json::from_str(r#""642245372237""#).unwrap();
        assert_eq!(serde_json::to_string(&text).unwrap(), r#""642245372237""#);

        let number: EventCounter = serde_json::from_str("642245372237").unwrap();
        assert_eq!(serde_json::to_string(&number).unwrap(), "642245372237");
    }

    // How many places a numeric counter is written with is a property of which counter it is, not of its value.
    // The original writes cpu_utilized with three and everything else with none, so a ratio that happens to land on 0.99 still goes out as 0.990 and an event count never grows a decimal point.
    #[test]
    fn places_come_from_the_counter_not_from_the_value() {
        let cpu: CpuCounter = serde_json::from_str("0.99").unwrap();
        assert_eq!(serde_json::to_string(&cpu).unwrap(), "0.990");

        let events: EventCounter = serde_json::from_str("49427").unwrap();
        assert_eq!(serde_json::to_string(&events).unwrap(), "49427");
    }

    #[test]
    fn unsupported_counters_read_as_zero_and_know_it() {
        let c = EventCounter::Text("<not supported>".to_owned());
        assert!((c.as_f64() - 0.0).abs() < f64::EPSILON);
        assert!(!c.is_measured());
        assert!(EventCounter::Text("21181".to_owned()).is_measured());
    }
}
