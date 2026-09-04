//! What a highlighted file or directory actually contains.
//!
//! Reading only, with no drawing in it — the same split `picker.rs` uses, so
//! the interesting decisions (is this text, how much of it do we read, what does
//! patching this directory in actually copy) are testable against real
//! temporary directories rather than through a rendered frame.
//!
//! Everything here is **bounded**. The patches screen points at arbitrary paths
//! in a home directory: a preview that read a 4 GB file or walked a whole
//! `node_modules` would hang the interface on a cursor move.

use std::path::{Path, PathBuf};

/// Most of a file we will read. Enough for any config, small enough that
/// landing on a database dump costs nothing.
const MAX_BYTES: u64 = 64 * 1024;

/// Most entries we will walk for a directory. A count that stops here is
/// reported as "at least", never as the truth.
const MAX_WALK: usize = 2000;

/// One run of text from an ANSI-coloured file, with the colours it asked for.
///
/// Plain `(u8, u8, u8)` rather than a ratatui `Color`: this module reads files
/// and does not draw, the same rule `picker.rs` follows.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Span {
    pub fg: Option<(u8, u8, u8)>,
    pub bg: Option<(u8, u8, u8)>,
    pub text: String,
}

/// What to show for the entry under the cursor.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Preview {
    /// A text file, already split into lines.
    Text { lines: Vec<String>, more: bool },
    /// A file whose escape sequences are *its own art* rather than noise —
    /// ANSI art, a coloured capture — kept with its colours instead of being
    /// stripped down to the letters.
    Ansi { lines: Vec<Vec<Span>>, more: bool },
    /// An archive, described the way a directory is: by what patching it in
    /// would bring.
    Archive {
        entries: usize,
        bytes: u64,
        sample: Vec<String>,
        capped: bool,
    },
    /// An image, which the pane draws rather than describes.
    ///
    /// The pixels are not held here: decoding belongs to whatever is about to
    /// draw them, and a cache of `DynamicImage`s keyed by cursor position is a
    /// good way to hold a hundred megabytes by accident. The dimensions are
    /// cheap and worth showing next to it.
    Image { width: u32, height: u32, bytes: u64 },
    /// A file that is not text. Previewing it is not useful, but its size is.
    Binary { bytes: u64 },
    /// A file with nothing in it — distinct from one we could not read.
    Empty,
    /// A directory, described by what patching it in would copy.
    Dir {
        files: usize,
        dirs: usize,
        bytes: u64,
        /// Paths relative to the directory, for a sense of what is in there.
        sample: Vec<String>,
        /// Whether the walk stopped early, making the counts a lower bound.
        capped: bool,
    },
    /// Unreadable, and why. A permission error and an empty file look the same
    /// on screen unless the reason is kept.
    Error(String),
}

impl Preview {
    /// Read whatever is at `path`.
    ///
    /// `lines` bounds how much text comes back, so a caller sized to the pane
    /// does not carry a thousand lines it will never draw.
    pub fn read(path: &Path, is_dir: bool, lines: usize) -> Preview {
        if is_dir {
            Self::read_dir(path, lines)
        } else if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(Self::is_archive)
        {
            Self::read_archive(path, lines)
        } else {
            Self::read_file(path, lines)
        }
    }

    /// Whether a name looks like an image this can decode.
    ///
    /// By extension, not by sniffing the header: the pane only needs to decide
    /// which *kind* of preview to build, and a mislabelled file simply fails to
    /// decode and falls back to being reported as binary.
    pub fn is_image(name: &str) -> bool {
        let ext = Path::new(name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        // Only what `image` is compiled with here — offering to preview a
        // format the decoder was not built for is a promise this cannot keep.
        matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif")
    }

    fn read_file(path: &Path, lines: usize) -> Preview {
        let meta = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(e) => return Preview::Error(e.to_string()),
        };
        if meta.len() == 0 {
            return Preview::Empty;
        }
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(Self::is_image)
        {
            // Dimensions only — the header, not the pixels. `image` reads just
            // enough of the file to answer, which is what makes this safe to do
            // on a cursor move over a directory full of photographs.
            if let Ok((width, height)) = image::image_dimensions(path) {
                return Preview::Image {
                    width,
                    height,
                    bytes: meta.len(),
                };
            }
            // Mislabelled or corrupt: fall through and describe it as binary
            // rather than claiming an image that cannot be drawn.
        }
        // Read a prefix, not the file: `read_to_string` on something large is
        // the whole problem, and a preview only ever shows the top.
        // `MAX_BYTES` bounds this before the cast, so `usize` always holds it
        // even where a pointer is 32 bits wide.
        let want = usize::try_from(MAX_BYTES.min(meta.len())).unwrap_or(usize::MAX);
        let mut buf = vec![0u8; want];
        let read = {
            use std::io::Read as _;
            match std::fs::File::open(path).and_then(|mut f| f.read(&mut buf)) {
                Ok(n) => n,
                Err(e) => return Preview::Error(e.to_string()),
            }
        };
        buf.truncate(read);

        // A NUL byte is the oldest and most reliable "this is not text" signal,
        // and it is what stops the pane rendering control characters.
        if buf.contains(&0) {
            return Preview::Binary { bytes: meta.len() };
        }
        let Ok(text) = String::from_utf8(buf) else {
            return Preview::Binary { bytes: meta.len() };
        };
        // An escape sequence in a text file is either noise to strip or the
        // file's own art. `ESC[` before any newline is the signal: prose does
        // not contain it, and ANSI art is nothing but.
        if text.contains("\u{1b}[") {
            let coloured: Vec<Vec<Span>> = text.lines().take(lines).map(Self::colourise).collect();
            if coloured
                .iter()
                .any(|l| l.iter().any(|s| s.fg.is_some() || s.bg.is_some()))
            {
                let more = text.lines().nth(lines).is_some()
                    || u64::try_from(read).unwrap_or(u64::MAX) < meta.len();
                return Preview::Ansi {
                    lines: coloured,
                    more,
                };
            }
        }
        let mut out: Vec<String> = text.lines().take(lines).map(Self::sanitise).collect();
        let more = text.lines().nth(lines).is_some()
            || u64::try_from(read).unwrap_or(u64::MAX) < meta.len();
        if out.is_empty() {
            out.push(String::new());
        }
        Preview::Text { lines: out, more }
    }

    /// Whether a name looks like an archive this can read.
    ///
    /// Matched on the whole lowercased name rather than `Path::extension`,
    /// because `.tar.gz` is two extensions and `extension()` only sees `gz`.
    pub fn is_archive(name: &str) -> bool {
        let lower = name.to_ascii_lowercase();
        [".zip", ".tar", ".tar.gz", ".tgz"]
            .iter()
            .any(|suffix| lower.ends_with(suffix))
    }

    /// What patching an archive in would bring, described like a directory.
    ///
    /// Bounded by the same `MAX_WALK` a directory walk is: an archive index can
    /// be enormous, and a listing that had to be read to the end before the
    /// pane drew would stall on a cursor move.
    fn read_archive(path: &Path, lines: usize) -> Preview {
        let lower = path.to_string_lossy().to_ascii_lowercase();
        // Decided once, from the same lowercased name `is_archive` matched on.
        let is_zip = lower.rsplit('.').next() == Some("zip");
        let is_plain_tar = lower.rsplit('.').next() == Some("tar");
        let file = match std::fs::File::open(path) {
            Ok(f) => f,
            Err(e) => return Preview::Error(e.to_string()),
        };
        let mut names: Vec<(String, u64)> = Vec::new();
        let mut capped = false;

        if is_zip {
            let Ok(mut zip) = zip::ZipArchive::new(file) else {
                return Preview::Binary {
                    bytes: std::fs::metadata(path).map_or(0, |m| m.len()),
                };
            };
            for i in 0..zip.len().min(MAX_WALK) {
                if let Ok(entry) = zip.by_index(i) {
                    if !entry.is_dir() {
                        names.push((entry.name().to_string(), entry.size()));
                    }
                }
            }
            capped = zip.len() > MAX_WALK;
        } else {
            // `.tar`, `.tar.gz` and `.tgz` differ only in whether a gzip layer
            // sits in front of the same format.
            let reader: Box<dyn std::io::Read> = if is_plain_tar {
                Box::new(file)
            } else {
                Box::new(flate2::read::GzDecoder::new(file))
            };
            let mut tar = tar::Archive::new(reader);
            let Ok(entries) = tar.entries() else {
                return Preview::Binary {
                    bytes: std::fs::metadata(path).map_or(0, |m| m.len()),
                };
            };
            for entry in entries.flatten() {
                if names.len() >= MAX_WALK {
                    capped = true;
                    break;
                }
                if entry.header().entry_type().is_file() {
                    if let Ok(p) = entry.path() {
                        names.push((p.display().to_string(), entry.size()));
                    }
                }
            }
        }

        if names.is_empty() && !capped {
            // Nothing readable in it: describe the file rather than claim an
            // empty archive, which a corrupt one would look like.
            return Preview::Binary {
                bytes: std::fs::metadata(path).map_or(0, |m| m.len()),
            };
        }
        let bytes = names.iter().map(|(_, n)| n).sum();
        let entries = names.len();
        let mut sample: Vec<String> = names.into_iter().map(|(n, _)| n).collect();
        sample.sort();
        sample.truncate(lines);
        Preview::Archive {
            entries,
            bytes,
            sample,
            capped,
        }
    }

    /// Split a line into coloured runs, reading the SGR sequences rather than
    /// discarding them.
    ///
    /// Only what ANSI art actually uses: reset, reverse, the basic and 256
    /// palettes, and truecolour. Anything else is consumed and ignored, which
    /// is the same outcome `sanitise` gives and never leaves stray digits on
    /// screen.
    fn colourise(line: &str) -> Vec<Span> {
        let mut out: Vec<Span> = Vec::new();
        let (mut fg, mut bg, mut reverse) = (None, None, false);
        let mut text = String::new();
        let mut chars = line.chars().peekable();

        let mut flush = |text: &mut String, fg, bg, reverse: bool| {
            if !text.is_empty() {
                let (fg, bg) = if reverse { (bg, fg) } else { (fg, bg) };
                out.push(Span {
                    fg,
                    bg,
                    text: std::mem::take(text),
                });
            }
        };

        while let Some(c) = chars.next() {
            match c {
                '\t' => text.push_str("    "),
                '\u{1b}' if chars.peek() == Some(&'[') => {
                    chars.next();
                    let mut params = String::new();
                    let mut final_byte = None;
                    for c in chars.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&c) {
                            final_byte = Some(c);
                            break;
                        }
                        params.push(c);
                    }
                    if final_byte != Some('m') {
                        continue;
                    }
                    flush(&mut text, fg, bg, reverse);
                    apply_sgr(&params, &mut fg, &mut bg, &mut reverse);
                }
                // Any other escape, and every remaining control character, is
                // dropped exactly as `sanitise` drops it.
                '\u{1b}' => {
                    chars.next();
                }
                c if c.is_control() => {}
                c => text.push(c),
            }
        }
        flush(&mut text, fg, bg, reverse);
        out
    }

    /// Make one line safe to put in a terminal cell.
    ///
    /// A file can be perfectly good UTF-8 with no NUL byte and still be full of
    /// **escape sequences** — ANSI art, a captured terminal session, a coloured
    /// log. Those bytes reach the cell buffer as ordinary characters and the
    /// terminal then *executes* them: the reported symptom was a preview
    /// overflowing into the file list and corrupting it.
    ///
    /// A CSI sequence is dropped whole rather than just its ESC, because
    /// leaving `[38;2;255;0;0m` behind as visible text is its own kind of
    /// wrong. Everything else that cannot be drawn goes too.
    fn sanitise(line: &str) -> String {
        let mut out = String::with_capacity(line.len());
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                // A tab is a real character with no width of its own; the pane
                // draws columns, so it becomes them.
                '\t' => out.push_str("    "),
                '\u{1b}' => {
                    // CSI (`ESC [`) and OSC (`ESC ]`) run to a terminator;
                    // anything else after ESC is a two-character sequence.
                    match chars.peek() {
                        Some('[') => {
                            chars.next();
                            // Parameters and intermediates, then one final byte
                            // in `@`..`~`.
                            for c in chars.by_ref() {
                                if ('\u{40}'..='\u{7e}').contains(&c) {
                                    break;
                                }
                            }
                        }
                        Some(']') => {
                            chars.next();
                            // BEL or ST ends it; a stray one just eats the rest
                            // of the line, which is the safe direction.
                            for c in chars.by_ref() {
                                if c == '\u{7}' || c == '\u{1b}' {
                                    break;
                                }
                            }
                        }
                        _ => {
                            chars.next();
                        }
                    }
                }
                c if c.is_control() => {}
                c => out.push(c),
            }
        }
        out
    }

    /// Walk a directory the way the patch does: everything under it,
    /// recursively, because the source becomes `<dir>/**/*`.
    fn read_dir(path: &Path, lines: usize) -> Preview {
        let mut files = 0usize;
        let mut dirs = 0usize;
        let mut bytes = 0u64;
        let mut sample: Vec<String> = Vec::new();
        let mut capped = false;
        let mut stack: Vec<PathBuf> = vec![path.to_path_buf()];
        let mut seen = 0usize;

        while let Some(dir) = stack.pop() {
            let entries = match std::fs::read_dir(&dir) {
                Ok(e) => e,
                // The top-level failure is the interesting one; a subdirectory
                // we cannot open just contributes nothing.
                Err(e) if dir == path => return Preview::Error(e.to_string()),
                Err(_) => continue,
            };
            for entry in entries.flatten() {
                seen += 1;
                if seen > MAX_WALK {
                    capped = true;
                    break;
                }
                let p = entry.path();
                // `file_type`, not `metadata`: a symlink is not followed, so a
                // loop cannot turn this walk into a hang.
                let Ok(ft) = entry.file_type() else { continue };
                if ft.is_dir() {
                    dirs += 1;
                    stack.push(p);
                } else {
                    files += 1;
                    bytes += entry.metadata().map_or(0, |m| m.len());
                    if sample.len() < lines {
                        if let Ok(rel) = p.strip_prefix(path) {
                            sample.push(rel.to_string_lossy().into_owned());
                        }
                    }
                }
            }
            if capped {
                break;
            }
        }
        sample.sort();
        Preview::Dir {
            files,
            dirs,
            bytes,
            sample,
            capped,
        }
    }
}

/// Apply one SGR parameter list to the running colour state.
fn apply_sgr(
    params: &str,
    fg: &mut Option<(u8, u8, u8)>,
    bg: &mut Option<(u8, u8, u8)>,
    reverse: &mut bool,
) {
    let nums: Vec<u32> = params
        .split(';')
        .map(|p| p.parse().unwrap_or(0))
        .collect::<Vec<_>>();
    let mut i = 0;
    while i < nums.len() {
        match nums[i] {
            0 => {
                *fg = None;
                *bg = None;
                *reverse = false;
            }
            7 => *reverse = true,
            27 => *reverse = false,
            30..=37 => *fg = Some(basic(nums[i] - 30, false)),
            90..=97 => *fg = Some(basic(nums[i] - 90, true)),
            39 => *fg = None,
            40..=47 => *bg = Some(basic(nums[i] - 40, false)),
            100..=107 => *bg = Some(basic(nums[i] - 100, true)),
            49 => *bg = None,
            n @ (38 | 48) => {
                let slot = if n == 38 { &mut *fg } else { &mut *bg };
                match nums.get(i + 1) {
                    // `38;2;r;g;b` — the only form ANSI art in the wild uses.
                    Some(2) => {
                        let c = (
                            u8::try_from(*nums.get(i + 2).unwrap_or(&0)).unwrap_or(255),
                            u8::try_from(*nums.get(i + 3).unwrap_or(&0)).unwrap_or(255),
                            u8::try_from(*nums.get(i + 4).unwrap_or(&0)).unwrap_or(255),
                        );
                        *slot = Some(c);
                        i += 4;
                    }
                    Some(5) => {
                        *slot = Some(xterm256(*nums.get(i + 2).unwrap_or(&0)));
                        i += 2;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        i += 1;
    }
}

/// The sixteen ANSI colours, in the values most terminals ship.
fn basic(index: u32, bright: bool) -> (u8, u8, u8) {
    const DIM: [(u8, u8, u8); 8] = [
        (0, 0, 0),
        (170, 0, 0),
        (0, 170, 0),
        (170, 85, 0),
        (0, 0, 170),
        (170, 0, 170),
        (0, 170, 170),
        (170, 170, 170),
    ];
    const BRIGHT: [(u8, u8, u8); 8] = [
        (85, 85, 85),
        (255, 85, 85),
        (85, 255, 85),
        (255, 255, 85),
        (85, 85, 255),
        (255, 85, 255),
        (85, 255, 255),
        (255, 255, 255),
    ];
    let table = if bright { BRIGHT } else { DIM };
    table[(index as usize).min(7)]
}

/// The xterm 256-colour cube and greyscale ramp.
#[allow(clippy::cast_possible_truncation)]
fn xterm256(n: u32) -> (u8, u8, u8) {
    match n {
        0..=7 => basic(n, false),
        8..=15 => basic(n - 8, true),
        16..=231 => {
            let n = n - 16;
            let step = |v: u32| if v == 0 { 0 } else { (v * 40 + 55) as u8 };
            (step(n / 36), step((n / 6) % 6), step(n % 6))
        }
        _ => {
            let v = ((n.min(255) - 232) * 10 + 8) as u8;
            (v, v, v)
        }
    }
}

/// Bytes as a person reads them.
pub fn format_bytes(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss)]
    match bytes {
        b if b < 1024 => format!("{b} B"),
        b if b < 1024 * 1024 => format!("{:.0} KiB", b as f64 / 1024.0),
        b if b < 1024 * 1024 * 1024 => format!("{:.1} MiB", b as f64 / (1024.0 * 1024.0)),
        b => format!("{:.1} GiB", b as f64 / (1024.0 * 1024.0 * 1024.0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("cozy-preview-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("sub/deeper")).unwrap();
        std::fs::write(root.join("config.toml"), "a = 1\nb = 2\nc = 3\n").unwrap();
        std::fs::write(root.join("sub/inner.txt"), "inner").unwrap();
        std::fs::write(root.join("sub/deeper/leaf.txt"), "leaf").unwrap();
        root
    }

    #[test]
    fn a_text_file_comes_back_as_lines() {
        let root = tree("text");
        let p = Preview::read(&root.join("config.toml"), false, 10);
        let Preview::Text { lines, more } = p else {
            panic!("{p:?}")
        };
        assert_eq!(lines, vec!["a = 1", "b = 2", "c = 3"]);
        assert!(!more);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_long_file_is_cut_and_says_so() {
        // The pane has a fixed height; carrying a thousand lines to draw ten of
        // them is pure cost.
        let root = tree("long");
        let path = root.join("long.txt");
        std::fs::write(&path, "line\n".repeat(500)).unwrap();
        let Preview::Text { lines, more } = Preview::read(&path, false, 10) else {
            panic!()
        };
        assert_eq!(lines.len(), 10);
        assert!(more, "the reader has to know there is more");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_huge_file_is_not_read_whole() {
        // The screen points at arbitrary paths in a home directory. Reading one
        // of these on a cursor move is the difference between a preview and a
        // hang.
        let root = tree("huge");
        let path = root.join("huge.bin");
        let big = usize::try_from(MAX_BYTES).unwrap() * 4;
        std::fs::write(&path, "x".repeat(big)).unwrap();
        let Preview::Text { lines, more } = Preview::read(&path, false, 5) else {
            panic!("a file of x's is still text")
        };
        assert!(more, "cut off, so it must say so");
        assert!(lines.len() <= 5);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_binary_file_reports_its_size_rather_than_its_bytes() {
        let root = tree("binary");
        let path = root.join("thing.bin");
        std::fs::write(&path, [0x7f, 0x45, 0x4c, 0x46, 0x00, 0x01, 0x02]).unwrap();
        assert_eq!(
            Preview::read(&path, false, 10),
            Preview::Binary { bytes: 7 }
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn invalid_utf8_counts_as_binary_rather_than_erroring() {
        let root = tree("utf8");
        let path = root.join("latin1.txt");
        std::fs::write(&path, [0xff, 0xfe, 0x41]).unwrap();
        assert!(matches!(
            Preview::read(&path, false, 10),
            Preview::Binary { .. }
        ));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn an_empty_file_is_not_an_error() {
        let root = tree("empty");
        let path = root.join("nothing");
        std::fs::write(&path, "").unwrap();
        assert_eq!(Preview::read(&path, false, 10), Preview::Empty);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_missing_path_explains_itself() {
        let p = Preview::read(Path::new("/definitely/not/here"), false, 10);
        assert!(matches!(p, Preview::Error(_)), "{p:?}");
    }

    #[test]
    fn a_directory_counts_what_patching_it_in_would_copy() {
        // Recursively, because the patch source becomes `<dir>/**/*` — a
        // top-level listing would understate it.
        let root = tree("dir");
        let p = Preview::read(&root, true, 10);
        let Preview::Dir {
            files,
            dirs,
            sample,
            capped,
            ..
        } = p
        else {
            panic!("{p:?}")
        };
        assert_eq!(files, 3, "config.toml, inner.txt, leaf.txt");
        assert_eq!(dirs, 2, "sub and deeper");
        assert!(!capped);
        assert!(
            sample.contains(&"sub/deeper/leaf.txt".to_string()),
            "paths are relative to the directory: {sample:?}"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_directory_walk_stops_and_says_the_count_is_a_floor() {
        let root = tree("wide");
        let many = root.join("many");
        std::fs::create_dir_all(&many).unwrap();
        for i in 0..(MAX_WALK + 50) {
            std::fs::write(many.join(format!("f{i}")), "x").unwrap();
        }
        let Preview::Dir { capped, files, .. } = Preview::read(&root, true, 5) else {
            panic!()
        };
        assert!(capped, "a huge tree must not be walked to the end");
        assert!(
            files <= MAX_WALK + 8,
            "and the walk really did stop: {files}"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn an_unreadable_directory_reports_itself() {
        let p = Preview::read(Path::new("/definitely/not/here"), true, 10);
        assert!(matches!(p, Preview::Error(_)), "{p:?}");
    }

    #[test]
    fn sizes_read_the_way_a_person_reads_them() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(2048), "2 KiB");
        assert_eq!(format_bytes(3 * 1024 * 1024), "3.0 MiB");
    }

    #[test]
    #[ignore = "reads real files on this machine"]
    fn against_the_reported_files() {
        let dir =
            std::path::PathBuf::from(std::env::var("HOME").unwrap()).join("Pictures/loadouts");
        let Ok(entries) = std::fs::read_dir(&dir) else {
            println!("no {} here", dir.display());
            return;
        };
        let mut checked = 0;
        for e in entries.flatten() {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("txt") {
                continue;
            }
            let Preview::Text { lines, .. } = Preview::read(&path, false, 20) else {
                panic!("{} did not read as text", path.display());
            };
            for l in &lines {
                assert!(
                    !l.chars().any(char::is_control),
                    "{}: a control character survived: {l:?}",
                    path.display()
                );
            }
            let widest = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
            println!(
                "  {:24} {} lines, widest {widest}",
                path.file_name().unwrap().to_string_lossy(),
                lines.len()
            );
            checked += 1;
        }
        println!("checked {checked} files");
        assert!(checked > 0, "expected some .txt files to check");
    }

    // -- ANSI art ----------------------------------------------------------

    #[test]
    fn ansi_art_keeps_its_colours_instead_of_being_stripped() {
        let root = tree("ansi");
        let path = root.join("art.txt");
        std::fs::write(&path, "\u{1b}[38;2;255;0;0mred\u{1b}[0m plain\n").unwrap();
        let Preview::Ansi { lines, .. } = Preview::read(&path, false, 10) else {
            panic!("a file that is nothing but colour is art, not prose")
        };
        assert_eq!(lines[0][0].fg, Some((255, 0, 0)));
        assert_eq!(lines[0][0].text, "red");
        assert_eq!(lines[0][1].fg, None, "reset clears it");
        assert_eq!(lines[0][1].text, " plain");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_file_with_escapes_but_no_colour_is_still_prose() {
        // A log with a stray cursor movement in it is text someone wants to
        // read, not a picture.
        let root = tree("not-art");
        let path = root.join("log.txt");
        std::fs::write(&path, "starting\u{1b}[2K done\n").unwrap();
        let p = Preview::read(&path, false, 10);
        assert!(matches!(p, Preview::Text { .. }), "{p:?}");
        let Preview::Text { lines, .. } = p else {
            unreachable!()
        };
        assert_eq!(lines[0], "starting done", "and the escape is gone");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn backgrounds_and_reverse_are_understood() {
        assert_eq!(
            Preview::colourise("\u{1b}[48;2;1;2;3mx")[0].bg,
            Some((1, 2, 3))
        );
        // Reverse swaps them, which is how art fills a cell with the
        // foreground colour.
        let spans = Preview::colourise("\u{1b}[38;2;9;9;9m\u{1b}[7mx");
        assert_eq!(spans[0].bg, Some((9, 9, 9)));
    }

    #[test]
    fn the_basic_and_256_palettes_resolve() {
        assert_eq!(Preview::colourise("\u{1b}[31mx")[0].fg, Some((170, 0, 0)));
        assert_eq!(Preview::colourise("\u{1b}[91mx")[0].fg, Some((255, 85, 85)));
        // 196 is the pure red of the 6×6×6 cube.
        assert_eq!(
            Preview::colourise("\u{1b}[38;5;196mx")[0].fg,
            Some((255, 0, 0))
        );
        // 232+ is the greyscale ramp.
        let grey = Preview::colourise("\u{1b}[38;5;240mx")[0].fg.unwrap();
        assert_eq!(grey.0, grey.1);
        assert_eq!(grey.1, grey.2);
    }

    #[test]
    fn no_control_character_survives_colourising() {
        // The whole reason the stripping exists: an escape reaching a cell is
        // executed by the terminal.
        let spans = Preview::colourise("a\u{1b}[38;2;1;1;1mb\u{7}c\u{1b}Zd");
        let text: String = spans.iter().map(|s| s.text.as_str()).collect();
        assert!(!text.chars().any(char::is_control), "{text:?}");
        assert_eq!(text, "abcd");
    }

    // -- archives ----------------------------------------------------------

    #[test]
    fn an_archive_is_described_by_what_it_would_bring() {
        let root = tree("zip");
        let path = root.join("bundle.zip");
        let file = std::fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts: zip::write::FileOptions<()> = zip::write::FileOptions::default();
        for name in ["a.toml", "nested/b.fish"] {
            zip.start_file(name, opts).unwrap();
            std::io::Write::write_all(&mut zip, b"hello").unwrap();
        }
        zip.finish().unwrap();

        let p = Preview::read(&path, false, 10);
        let Preview::Archive {
            entries,
            sample,
            bytes,
            capped,
        } = p
        else {
            panic!("{p:?}")
        };
        assert_eq!(entries, 2);
        assert_eq!(
            bytes, 10,
            "the sum of what is inside, not the zip's own size"
        );
        assert!(sample.contains(&"nested/b.fish".to_string()), "{sample:?}");
        assert!(!capped);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_gzipped_tar_reads_too() {
        let root = tree("tgz");
        let path = root.join("bundle.tar.gz");
        let file = std::fs::File::create(&path).unwrap();
        let enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut tar = tar::Builder::new(enc);
        tar.append_path_with_name(root.join("config.toml"), "config.toml")
            .unwrap();
        tar.into_inner().unwrap().finish().unwrap();

        let p = Preview::read(&path, false, 10);
        let Preview::Archive {
            entries, sample, ..
        } = p
        else {
            panic!("{p:?}")
        };
        assert_eq!(entries, 1);
        assert_eq!(sample, vec!["config.toml"]);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_corrupt_archive_falls_back_to_being_described() {
        // A `.zip` that is not one is ordinary, not a fault — and must not be
        // reported as an empty archive, which is a different thing.
        let root = tree("bad-zip");
        let path = root.join("lying.zip");
        std::fs::write(&path, "not a zip at all").unwrap();
        assert!(matches!(
            Preview::read(&path, false, 10),
            Preview::Binary { .. }
        ));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
