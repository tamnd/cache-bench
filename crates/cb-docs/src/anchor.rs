//! GitHub's heading anchors, including the numbering it appends to repeated ones.
//!
//! Each chart index has the same nine headings under each of four pipeline blocks, so almost every link in it points at a heading that appears four times. GitHub resolves that by numbering the repeats in document order, and getting the numbering wrong produces a document whose links all land in the first block. The original maintains those suffixes by hand. Here they come out of a counter fed the same headings, in the same order, as the document itself.

use std::collections::BTreeMap;

/// The anchor GitHub gives a heading the first time it sees it.
///
/// Lower case, spaces turned into hyphens, everything else that is not a letter, a digit, a hyphen or an underscore dropped. That last rule is why `Latency 99.9th Percentile` becomes `latency-999th-percentile` rather than growing a hyphen where the dot was. Only the ASCII case mapping is applied, which is all these headings need.
#[must_use]
pub fn slug(heading: &str) -> String {
    heading
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            '-' | '_' => Some(c),
            c if c.is_alphanumeric() => Some(c.to_ascii_lowercase()),
            _ => None,
        })
        .collect()
}

/// The headings of one document, in the order they were written.
#[derive(Debug, Default)]
pub struct Anchors {
    /// How many times each slug has been handed out.
    seen: BTreeMap<String, usize>,
}

impl Anchors {
    /// The anchor for the next heading in the document.
    ///
    /// The first `Throughput` gets `throughput` and the ones after it get `throughput-1`, `throughput-2` and so on, so this has to be called once per heading in the order the headings appear. Calling it for a heading nothing links to still matters, because a repeat of that heading later on is numbered as if the earlier one had been counted.
    pub fn assign(&mut self, heading: &str) -> String {
        let base = slug(heading);
        let seen = self.seen.entry(base.clone()).or_insert(0);
        let anchor = if *seen == 0 {
            base
        } else {
            format!("{base}-{seen}")
        };
        *seen += 1;
        anchor
    }
}

#[cfg(test)]
mod tests {
    use super::{Anchors, slug};

    #[test]
    fn punctuation_is_dropped_rather_than_replaced() {
        assert_eq!(
            slug("Latency 99.9th Percentile"),
            "latency-999th-percentile"
        );
        assert_eq!(
            slug("Latency 99.99th Percentile"),
            "latency-9999th-percentile"
        );
        assert_eq!(slug("CPU Cycles"), "cpu-cycles");
        assert_eq!(slug("Latency MAX"), "latency-max");
    }

    // These are the suffixes the original wrote by hand, which is what makes them worth asserting.
    #[test]
    fn repeats_are_numbered_from_one_in_document_order() {
        let mut anchors = Anchors::default();
        assert_eq!(anchors.assign("Throughput"), "throughput");
        assert_eq!(anchors.assign("CPU Cycles"), "cpu-cycles");
        assert_eq!(anchors.assign("Throughput"), "throughput-1");
        assert_eq!(anchors.assign("Throughput"), "throughput-2");
        assert_eq!(anchors.assign("CPU Cycles"), "cpu-cycles-1");
    }
}
