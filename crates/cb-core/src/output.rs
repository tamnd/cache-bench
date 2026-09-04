//! `output.json`, which is every chosen run in one file.
//!
//! This is the only file the chart layer reads, and it is the file that gets published next to the charts so that anybody can redraw them without trusting us.
//! It is also the file the parity test uses in both directions, so its bytes are the original's exactly.
//!
//! The layout is not something a serialiser produces.
//! The original builds it by pasting run files together as text: it widens the two space indent to four with a blind search and replace, rewrites the closing brace so it sits one level in, wraps each one in an object carrying the filename, and separates them with `},{` on one line.
//! What comes out is valid JSON with an indentation scheme no formatter would choose, and reproducing it means doing the same thing rather than describing it to a pretty printer.

use serde::{Deserialize, Serialize};

use crate::name::{BadName, RunName};
use crate::run::Run;

/// One entry, which is one chosen run plus the name of the file it came from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// The result filename, carried as text.
    ///
    /// Kept as it was written rather than as a parsed `RunName`, so that a file naming a cache server this build has never heard of still loads and still round trips.
    /// Call [`Entry::name`] to take it apart.
    pub file: String,
    /// The run itself.
    pub data: Run,
}

impl Entry {
    /// Take the filename apart.
    ///
    /// # Errors
    ///
    /// If the name is not a result filename, or names a cache server this build does not know.
    pub fn name(&self) -> Result<RunName, BadName> {
        self.file.parse()
    }
}

/// The whole file.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Output {
    /// Every chosen run, in filename order, which is the order the original's directory listing produces.
    pub entries: Vec<Entry>,
}

impl Output {
    /// Read an `output.json`.
    ///
    /// # Errors
    ///
    /// If the text is not JSON, or is JSON of the wrong shape.
    pub fn parse(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }

    /// Write an `output.json`, byte for byte as the original writes it.
    #[must_use]
    pub fn emit(&self) -> String {
        let mut out = String::from("[\n");
        for (i, entry) in self.entries.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str("{\n  \"file\": \"");
            out.push_str(&entry.file);
            out.push_str("\",\n  \"data\": ");
            out.push_str(&reindent(&entry.data.emit()));
            out.push('}');
        }
        out.push_str("\n]\n");
        out
    }
}

/// Push a run file's text in by one level, the way the original does it.
///
/// Two spaces become four everywhere they appear, and then the closing brace, which that pass left at column zero, is moved in by two.
///
/// The search and replace is blind, so a two space run inside a string value would be widened as well.
/// That is a real hazard rather than a theoretical one, because one of the fields is whatever the server printed when asked for its version, and nothing stops a server printing two spaces.
/// None of the seven do, so the original has never hit it and neither will we, but the behaviour here is the original's rather than a corrected version of it, because this is the half of the code whose job is to agree.
fn reindent(run: &str) -> String {
    let widened = run.replace("  ", "    ");
    match widened.strip_suffix("\n}\n") {
        Some(head) => format!("{head}\n  }}\n"),
        None => widened,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{Entry, Output};
    // Three entries lifted out of the original's published output.json without changing a byte, which is what the whole format exists to be able to do.
    use crate::golden::COMBINED as GOLDEN;
    use crate::name::{Chosen, Slot};
    use crate::run::Run;

    #[test]
    fn the_originals_output_round_trips_byte_for_byte() {
        let out = Output::parse(GOLDEN).unwrap();
        assert_eq!(out.entries.len(), 3);
        assert_eq!(out.emit(), GOLDEN);
    }

    #[test]
    fn entries_carry_a_filename_that_parses() {
        let out = Output::parse(GOLDEN).unwrap();
        let first = out.entries[0].name().unwrap();
        assert_eq!(first.threads, 1);
        assert_eq!(first.slot, Slot::Chosen(Chosen::Average));
        let last = out.entries[2].name().unwrap();
        assert_eq!(last.threads, 8);
        assert_eq!(last.pipeline, 50);
        assert!(last.perf);
        assert_eq!(last.slot, Slot::Chosen(Chosen::Worst));
    }

    // An unknown cache server has to load rather than fail the file, because a results directory written by a future build of this harness should still be readable by an older one.
    #[test]
    fn an_unknown_cache_still_loads_and_still_round_trips() {
        let out = Output::parse(GOLDEN).unwrap();
        let mut odd = out.entries[0].clone();
        odd.file = odd.file.replace("dragonfly", "somethingelse");
        assert!(odd.name().is_err());
        let one = Output {
            entries: vec![odd.clone()],
        };
        assert_eq!(Output::parse(&one.emit()).unwrap(), one);
    }

    // The empty case is what combine writes for a directory with nothing chosen in it yet, and it is not the empty JSON array anyone would write by hand.
    #[test]
    fn an_empty_output_is_still_the_originals_empty_output() {
        assert_eq!(Output::default().emit(), "[\n\n]\n");
    }

    // The real proof, against the whole 1.7 MB file rather than three entries of it.
    // That file is not committed, because raw measurement data does not go in this repository, so this runs on demand against a checkout of the original.
    // Ignored by default. `cache-bench verify --against` covers the same ground without an environment variable, and this stays because it is the one that fails inside `cargo test`.
    #[test]
    #[ignore = "needs a checkout of the original, see CB_PARITY_OUTPUT"]
    fn the_whole_published_output_round_trips() {
        let path = std::env::var("CB_PARITY_OUTPUT")
            .map_err(|_| "set CB_PARITY_OUTPUT to the original's results/output.json")
            .unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let out = Output::parse(&text).unwrap();
        assert_eq!(out.entries.len(), 2304);
        let ours = out.emit();
        assert_eq!(ours, text);
        // The other half of the gate is that the original's graph reads a file we wrote, so set this to a path and point graph at the directory it lands in.
        if let Ok(path) = std::env::var("CB_PARITY_EMIT") {
            std::fs::write(path, &ours).unwrap();
        }
    }

    // Every entry has to survive the trip through the wider indent and come back as the same run, since the run file and the entry are the two places the same measurement is written down.
    #[test]
    fn an_entry_holds_the_same_run_the_run_file_holds() {
        use crate::golden::CHOSEN;

        let run = Run::parse(CHOSEN).unwrap();
        let out = Output {
            entries: vec![Entry {
                file: "bench_dragonfly-threads_1-pipeline_1-perf_yes-run_median.json".to_owned(),
                data: run.clone(),
            }],
        };
        let back = Output::parse(&out.emit()).unwrap();
        assert_eq!(back.entries[0].data, run);
        assert_eq!(back.entries[0].data.emit(), CHOSEN);
    }
}
