//! Manga support, first pieces (experimental).
//!
//! The app uses [`reading`] for reading detection (YACReader).
//!
//! - [`name`]: a file name parser for comic files (`.cbz`, `.cbr`, `.cb7`, `.zip`, `.pdf`,
//!   `.epub`) and their folders: series, volume, chapter, group, language, year.
//! - [`yacreader`]: a read-only reader of YACReader's library databases. It reports what the user
//!   opened lately, how far they got, and whether YACReader calls it read.
//! - [`reading`]: what was read → a manga on the list, the offer, and the library watcher.
//! - [`chapter_read`]: the one rule that says when Kotora counts a chapter as read.

pub mod name;
pub mod openfile;
pub mod reading;
pub mod resume;
pub mod viewer;
pub mod yacreader;

/// Whether a chapter counts as read.
///
/// With a known page count, the page decides: the last page or the one before it
/// (`current_page >= num_pages - 1`; the last page is often a credits or an ad page people don't
/// flip to). YACReader's own "read" flag counts only when the page count is unknown: it stays set
/// from an earlier reading, and YACReader sends it again with a fresh time when the viewer moves to
/// the next comic (live test, 2026-11-17), so a volume left after 6 s looked "read to the end".
///
/// Pages are counted from 1, as YACReader does (`currentPage` is 1 for a book that was just
/// opened).
pub fn chapter_read(read_flag: bool, current_page: i64, num_pages: i64) -> bool {
    if num_pages > 0 {
        current_page >= num_pages - 1
    } else {
        read_flag
    }
}

#[cfg(test)]
mod tests {
    use super::chapter_read;

    #[test]
    fn the_page_beats_a_stale_flag() {
        assert!(
            !chapter_read(true, 1, 200),
            "set from an earlier reading; page 1 now"
        );
        assert!(!chapter_read(true, 12, 200));
        assert!(chapter_read(true, 199, 200));
        assert!(chapter_read(true, 0, 0), "no page count: the flag");
    }

    #[test]
    fn the_last_two_pages_count() {
        assert!(chapter_read(false, 20, 20));
        assert!(chapter_read(false, 19, 20));
        assert!(!chapter_read(false, 18, 20));
        assert!(chapter_read(false, 25, 20)); // past the end (a bad count): still read
    }

    #[test]
    fn an_unknown_page_count_counts_only_by_the_flag() {
        assert!(!chapter_read(false, 5, 0));
        assert!(!chapter_read(false, 5, -1));
    }

    #[test]
    fn a_one_page_file() {
        assert!(chapter_read(false, 1, 1));
    }
}
