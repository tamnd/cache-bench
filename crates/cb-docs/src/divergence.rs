//! The divergences table, as it appears in a generated results README.
//!
//! Everything this port does that the original does not is written up in `divergences.md`, which is prose and is where the reasoning lives. A reader who has followed a link to a chart is not going to read it, so a short table of the same list goes into the README next to the charts, with a link into the long version for each row.
//!
//! Two lists of the same thing drift. The guard against that is a test that reads `divergences.md`, pulls out its headings and asserts that they are these rows, in this order, with these titles. Adding a divergence to one and not the other fails the build.

use std::fmt::Write as _;

use crate::anchor::slug;

/// One row of the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Divergence {
    /// The identifier, which is what the rest of the project refers to it by.
    pub id: &'static str,
    /// The heading it has in `divergences.md`, after the identifier.
    pub title: &'static str,
    /// What it does, in one line.
    pub effect: &'static str,
    /// Whether it changes a number that ends up on a chart. The ones that do are the ones that make a bar here differ from the same bar in the original.
    pub moves: bool,
}

/// Every divergence, in the order `divergences.md` has them.
pub const DIVERGENCES: [Divergence; 19] = [
    Divergence {
        id: "D1 to D4",
        title: "the statistics",
        effect: "Four defects in the original's run selection are corrected. `--compat=upstream` reproduces them exactly.",
        moves: true,
    },
    Divergence {
        id: "D5",
        title: "an added spread object",
        effect: "Chosen files carry the interquartile range, the standard deviation and the coefficient of variation. Nothing plots them.",
        moves: false,
    },
    Divergence {
        id: "D6",
        title: "fonts",
        effect: "Jost and DejaVu Sans, embedded in the binary, instead of Futura and Verdana, which only resolve on a Mac.",
        moves: false,
    },
    Divergence {
        id: "D7",
        title: "a --threads flag on yo",
        effect: "The x axis of every chart is a thread count, so a server with no way to set one cannot be plotted.",
        moves: true,
    },
    Divergence {
        id: "D8",
        title: "hardware profiles",
        effect: "Core pinning, thread sweep, memory limit and client count are data rather than constants, so the harness runs on a box that is not a c8g.8xlarge.",
        moves: true,
    },
    Divergence {
        id: "D9",
        title: "no Python",
        effect: "The chart engine lays out the axes and draws the pixels itself, so the same numbers produce the same bytes everywhere.",
        moves: false,
    },
    Divergence {
        id: "D10",
        title: "an added subject",
        effect: "`yo` and `rugo` are a seventh and an eighth engine the original does not have. Everything the original measures is still measured.",
        moves: false,
    },
    Divergence {
        id: "D11",
        title: "unsupported perf counters",
        effect: "A counter the machine cannot measure leaves the bar off the chart rather than drawing it at zero.",
        moves: true,
    },
    Divergence {
        id: "D12",
        title: "Dragonfly's memory limit",
        effect: "Dragonfly gets the same limit as everything else. The original's arithmetic gives it one gigabyte less than the rest.",
        moves: true,
    },
    Divergence {
        id: "D13",
        title: "the x tick offset",
        effect: "The thread count under a group of bars is centred under the group for any number of engines rather than for exactly six.",
        moves: false,
    },
    Divergence {
        id: "D14",
        title: "one canvas size",
        effect: "Every chart is the same size, so two of them can be flipped between without everything shifting sideways.",
        moves: false,
    },
    Divergence {
        id: "D15",
        title: "the provenance stamp",
        effect: "Every chart drawn from real measurements says which host produced it.",
        moves: false,
    },
    Divergence {
        id: "D16",
        title: "the chart indexes",
        effect: "Both indexes are generated, they cover all 154 charts rather than 120, and every image has a real alt text.",
        moves: false,
    },
    Divergence {
        id: "D17",
        title: "an added host record",
        effect: "Each results directory carries a `host.json` saying what it was measured on, with nothing in it that names the machine.",
        moves: false,
    },
    Divergence {
        id: "D18",
        title: "a generated results README",
        effect: "The methodology and the version table are generated from the results rather than typed, so they cannot disagree with the data above them.",
        moves: false,
    },
    Divergence {
        id: "D19",
        title: "the numbers carry their own caveat",
        effect: "What these numbers may and may not be used for is emitted next to the charts rather than kept in a document nobody following a chart link will open.",
        moves: false,
    },
    Divergence {
        id: "D20",
        title: "pinned before exec, and stopped as a group",
        effect: "The CPU pin is applied in the server itself rather than by wrapping it in taskset, and every run confirms its process group is gone before the next one starts.",
        moves: false,
    },
    Divergence {
        id: "D21",
        title: "counters read machine readable, and utilisation computed",
        effect: "perf is asked for its comma separated output rather than its human table, and CPUs utilized is computed from `task-clock` and a measured duration rather than scraped out of a comment column.",
        moves: false,
    },
    Divergence {
        id: "D22",
        title: "a colour belongs to a server",
        effect: "A bar colour is looked up by which server it is rather than by where the server sorted in that sweep, so the same engine is the same colour in every chart.",
        moves: false,
    },
];

/// Where the long version of a divergence lives, from a results directory two levels down.
fn link(row: &Divergence) -> String {
    format!(
        "../../divergences.md#{}",
        slug(&format!("{}, {}", row.id, row.title))
    )
}

/// The table.
#[must_use]
pub fn table() -> String {
    let mut out =
        String::from("| Divergence | What it does | Moves a plotted number |\n|---|---|---|\n");
    for row in &DIVERGENCES {
        let moves = if row.moves { "yes" } else { "no" };
        let _ = writeln!(
            out,
            "| [{}, {}]({}) | {} | {moves} |",
            row.id,
            row.title,
            link(row),
            row.effect
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{DIVERGENCES, table};

    /// The long version, which this table is a summary of.
    const PROSE: &str = include_str!("../../../divergences.md");

    /// Every `## D` heading in it, split into the identifier and the title.
    fn headings() -> Vec<(String, String)> {
        PROSE
            .lines()
            .filter_map(|line| line.strip_prefix("## "))
            .filter(|heading| heading.starts_with('D'))
            .filter_map(|heading| heading.split_once(", "))
            .map(|(id, title)| (id.to_owned(), title.to_owned()))
            .collect()
    }

    // The whole point of the table. Add a divergence to one list and not the other and this fails.
    #[test]
    fn the_table_is_the_same_list_as_the_prose_in_the_same_order() {
        let prose = headings();
        let table: Vec<(String, String)> = DIVERGENCES
            .iter()
            .map(|row| (row.id.to_owned(), row.title.to_owned()))
            .collect();
        assert_eq!(table, prose);
    }

    // A link into a heading that is not there is a link that lands at the top of the file, which looks like it worked.
    #[test]
    fn every_row_links_to_a_heading_that_exists() {
        for row in &DIVERGENCES {
            let heading = format!("## {}, {}", row.id, row.title);
            assert!(
                PROSE.contains(&heading),
                "{heading} is not in divergences.md"
            );
        }
    }

    #[test]
    fn the_table_is_markdown_with_a_row_per_divergence() {
        let text = table();
        assert_eq!(text.lines().count(), DIVERGENCES.len() + 2);
        assert!(
            text.contains(
                "| [D1 to D4, the statistics](../../divergences.md#d1-to-d4-the-statistics) |"
            ),
            "{text}"
        );
    }
}
