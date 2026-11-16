//! Manga support, first pieces (experimental).
//!
//! Nothing here is wired into the app yet. There is no UI, no list sync and no thread.
//!
//! - [`name`]: a file name parser for comic files (`.cbz`, `.cbr`, `.cb7`, `.zip`, `.pdf`,
//!   `.epub`) and their folders: series, volume, chapter, group, language, year.
//! - [`yacreader`]: a read-only reader of YACReader's library databases. It reports what the user
//!   opened lately, how far they got, and whether YACReader calls it read.
//! - [`chapter_read`]: the one rule that says when Kotora counts a chapter as read.

pub mod name;
pub mod yacreader;

/// Whether a chapter counts as read.
///
/// Yes when the reader's own "read" flag is set, or when the user is on the last page or the one
/// before it (`current_page >= num_pages - 1`). The one-page slack is on purpose: the last page of
/// a chapter is often a credits or an ad page that people don't flip to.
///
/// Pages are counted from 1, as YACReader does (`currentPage` is 1 for a book that was just
/// opened). An unknown page count (`num_pages <= 0`) never counts, only the flag does.
pub fn chapter_read(read_flag: bool, current_page: i64, num_pages: i64) -> bool {
    read_flag || (num_pages > 0 && current_page >= num_pages - 1)
}

#[cfg(test)]
mod tests {
    use super::chapter_read;

    #[test]
    fn the_flag_wins() {
        assert!(chapter_read(true, 1, 200));
        assert!(chapter_read(true, 0, 0));
    }

    #[test]
    fn the_last_two_pages_count() {
        assert!(chapter_read(false, 20, 20));
        assert!(chapter_read(false, 19, 20));
        assert!(!chapter_read(false, 18, 20));
        assert!(chapter_read(false, 25, 20)); // past the end (a bad count): still read
    }

    #[test]
    fn an_unknown_page_count_never_counts() {
        assert!(!chapter_read(false, 5, 0));
        assert!(!chapter_read(false, 5, -1));
    }

    #[test]
    fn a_one_page_file() {
        assert!(chapter_read(false, 1, 1));
    }
}
