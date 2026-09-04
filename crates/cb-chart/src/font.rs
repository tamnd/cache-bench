//! The three faces a chart is drawn with, carried in the binary.
//!
//! The original sets `plt.rcParams['font.family'] = "Futura"` and then asks matplotlib for bold on the title and both axis labels, and for Verdana on the small gray numbers down the left of a logarithmic chart. Futura and Verdana are neither redistributable nor present anywhere but macOS, so this port substitutes Jost and DejaVu Sans and puts all three cuts in the executable.
//!
//! Embedding is the point rather than a convenience. A chart drawn against whatever the host had installed is a chart nobody else can reproduce, and the hash manifest that says three platforms drew the same PNG only means something if the letter shapes cannot move underneath it. See D6 in `divergences.md`.

use skrifa::FontRef;
use skrifa::raw::ReadError;

/// One of the three faces, named for what it draws rather than for its weight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Face {
    /// Tick labels on both axes and the legend entries, which the original leaves at Futura's normal weight.
    Body,
    /// The title and both axis labels, which the original asks for with `fontweight='bold'`.
    Heading,
    /// The quarter decade labels in the margin of a logarithmic chart, eight point and gray, which the original draws in Verdana.
    Gutter,
}

impl Face {
    /// All three.
    pub const ALL: [Self; 3] = [Self::Body, Self::Heading, Self::Gutter];

    /// The bytes of the face, as they sit in `crates/cb-chart/assets/fonts`.
    #[must_use]
    pub const fn bytes(self) -> &'static [u8] {
        match self {
            Self::Body => include_bytes!("../assets/fonts/jost/Jost-400-Book.ttf"),
            Self::Heading => include_bytes!("../assets/fonts/jost/Jost-700-Bold.ttf"),
            Self::Gutter => include_bytes!("../assets/fonts/dejavu/DejaVuSans.ttf"),
        }
    }

    /// The family name as the font itself gives it.
    ///
    /// The asterisk on Jost is not a footnote marker, it is part of the name the foundry chose.
    #[must_use]
    pub const fn family(self) -> &'static str {
        match self {
            Self::Body | Self::Heading => "Jost*",
            Self::Gutter => "DejaVu Sans",
        }
    }

    /// The full name of the cut, which is what the renderer registers it under.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Body => "Jost* Book",
            Self::Heading => "Jost* Bold",
            Self::Gutter => "DejaVu Sans",
        }
    }

    /// The weight the face is cut at, on the usual 100 to 900 scale.
    #[must_use]
    pub const fn weight(self) -> u16 {
        match self {
            Self::Body | Self::Gutter => 400,
            Self::Heading => 700,
        }
    }

    /// The file the bytes came from, relative to `assets/fonts`.
    #[must_use]
    pub const fn file(self) -> &'static str {
        match self {
            Self::Body => "jost/Jost-400-Book.ttf",
            Self::Heading => "jost/Jost-700-Bold.ttf",
            Self::Gutter => "dejavu/DejaVuSans.ttf",
        }
    }

    /// The face in the original that this one stands in for.
    #[must_use]
    pub const fn stands_in_for(self) -> &'static str {
        match self {
            Self::Body => "Futura",
            Self::Heading => "Futura Bold",
            Self::Gutter => "Verdana",
        }
    }

    /// The SHA-256 of `bytes`, checked by a test.
    ///
    /// A font file swapped for a different cut of the same family would change every chart in the manifest and would otherwise change nothing a reviewer can see, so the digest is written down where a diff has to touch it.
    #[must_use]
    pub const fn digest(self) -> &'static str {
        match self {
            Self::Body => "60de951651870fd2dbbd099a96f9321f183171cd7d73047905787f37d2ec2a13",
            Self::Heading => "1a2bc42d83ee2debd2c1b528a7eb29f977f226ea586d232edef478aa8aa0e87f",
            Self::Gutter => "7da195a74c55bef988d0d48f9508bd5d849425c1770dba5d7bfc6ce9ed848954",
        }
    }

    /// Parse the face.
    ///
    /// Cheap enough to call per chart, since it borrows the embedded bytes and reads the table directory rather than copying anything.
    ///
    /// # Errors
    ///
    /// If the embedded bytes are not a font the parser understands, which can only happen if the file in `assets/fonts` was replaced.
    pub fn load(self) -> Result<FontRef<'static>, ReadError> {
        FontRef::new(self.bytes())
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "a font that will not parse is a failed test"
)]
mod tests {
    use super::Face;
    use sha2::{Digest, Sha256};
    use skrifa::MetadataProvider;
    use skrifa::instance::{LocationRef, Size};
    use skrifa::raw::TableProvider as _;
    use skrifa::string::StringId;
    use std::fmt::Write as _;

    fn string(face: Face, id: StringId) -> String {
        let font = face.load().unwrap();
        font.localized_strings(id)
            .english_or_first()
            .unwrap()
            .chars()
            .collect()
    }

    #[test]
    fn every_face_parses() {
        for face in Face::ALL {
            let font = face.load().unwrap();
            let metrics = font.metrics(Size::unscaled(), LocationRef::default());
            assert!(metrics.units_per_em > 0, "{face:?} has no head table");
            assert!(font.maxp().unwrap().num_glyphs() > 0, "{face:?} is empty");
        }
    }

    // Everything that reaches a chart: the titles and axis labels that `spec` builds, the cache server names in the legend, and the digits and separators of a tick label.
    const ALPHABET: &str = concat!(
        "0123456789,.+-() /",
        "ABCDEFGHIJKLMNOPQRSTUVWXYZ",
        "abcdefghijklmnopqrstuvwxyz",
    );

    #[test]
    fn every_face_has_the_characters_a_chart_needs() {
        for face in Face::ALL {
            let font = face.load().unwrap();
            let charmap = font.charmap();
            for c in ALPHABET.chars() {
                assert!(
                    charmap.map(c).is_some(),
                    "{face:?} has no glyph for {c:?}, which a chart asks for"
                );
            }
        }
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().fold(String::new(), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
    }

    // Not a style check. If a font file is ever replaced this is the test that says so, rather than a hash manifest full of new numbers with no explanation.
    #[test]
    fn the_embedded_bytes_are_the_files_that_were_reviewed() {
        for face in Face::ALL {
            let got = hex(&Sha256::digest(face.bytes()));
            assert_eq!(got, face.digest(), "{} has changed", face.file());
        }
    }

    // The names here are claims about files, so they are checked against what the files say about themselves rather than left as comments.
    #[test]
    fn every_face_is_the_font_it_says_it_is() {
        for face in Face::ALL {
            assert_eq!(string(face, StringId::FAMILY_NAME), face.family());
            assert_eq!(string(face, StringId::FULL_NAME), face.name());
        }
    }

    // One family, two cuts. The original gets its bold by asking matplotlib rather than by naming a second font, and a bold smeared out of the regular is not the same shape on two machines, so the second cut is a second file.
    #[test]
    fn the_bold_is_a_real_bold() {
        assert_ne!(Face::Body.bytes(), Face::Heading.bytes());
        assert_eq!(Face::Body.family(), Face::Heading.family());
        for face in Face::ALL {
            let font = face.load().unwrap();
            // A tolerance rather than equality because the weight axis is a float, and half a step is far inside the gap between one weight and the next.
            let cut = font.attributes().weight.value() - f32::from(face.weight());
            assert!(cut.abs() < 0.5, "{face:?} is cut at the wrong weight");
        }
    }

    #[test]
    fn every_face_is_its_own_entry() {
        let mut names: Vec<_> = Face::ALL.iter().map(|f| f.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), Face::ALL.len());
    }
}
