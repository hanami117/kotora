use super::*;

/// One line per parse: `Series :: v3 c21.5 "chapter title" g=Group l=en y=2019 x=A,B one`.
/// Only the parts that are set show up.
fn show(p: &Parsed) -> String {
    let mut parts = Vec::new();
    if let Some(v) = p.volume {
        parts.push(format!("v{v}"));
    }
    if let Some(c) = p.chapter {
        parts.push(format!("c{c}"));
    }
    if let Some(t) = &p.chapter_title {
        parts.push(format!("\"{t}\""));
    }
    if let Some(g) = &p.group {
        parts.push(format!("g={g}"));
    }
    if let Some(l) = &p.language {
        parts.push(format!("l={l}"));
    }
    if let Some(y) = p.year {
        parts.push(format!("y={y}"));
    }
    if !p.extras.is_empty() {
        parts.push(format!("x={}", p.extras.join(",")));
    }
    if p.oneshot {
        parts.push("one".to_string());
    }
    format!("{} :: {}", p.series, parts.join(" "))
}

fn check_files(table: &[(&str, &str)]) {
    let mut bad = Vec::new();
    for (input, want) in table {
        let got = show(&parse_file_name(input));
        if got.trim_end() != want.trim_end() && got != *want {
            bad.push(format!("{input}\n   want {want}\n   got  {got}"));
        }
    }
    assert!(bad.is_empty(), "\n{}", bad.join("\n"));
}

fn check_paths(table: &[(&str, &str)]) {
    let mut bad = Vec::new();
    for (input, want) in table {
        let got = show(&parse_path(input));
        if got != *want {
            bad.push(format!("{input}\n   want {want}\n   got  {got}"));
        }
    }
    assert!(bad.is_empty(), "\n{}", bad.join("\n"));
}

#[test]
fn volume_forms() {
    check_files(&[
        ("Lantern Road v03.cbz", "Lantern Road :: v3"),
        ("Lantern Road V03.cbz", "Lantern Road :: v3"),
        ("Lantern Road Vol.3.cbz", "Lantern Road :: v3"),
        ("Lantern Road Vol. 3.cbz", "Lantern Road :: v3"),
        ("Lantern Road Vol 03.cbz", "Lantern Road :: v3"),
        ("Lantern Road Volume 03.cbz", "Lantern Road :: v3"),
        ("Lantern Road - Volume 3.cbz", "Lantern Road :: v3"),
        ("Lantern Road v01-03.cbz", "Lantern Road :: v1-3"),
        ("Lantern Road Vol.01-03.cbz", "Lantern Road :: v1-3"),
        ("Lantern Road v103.cbz", "Lantern Road :: v103"),
        ("Lantern Road (v03).cbz", "Lantern Road :: v3"),
        ("Lantern Road [Vol. 3].cbz", "Lantern Road :: v3"),
    ]);
}

#[test]
fn chapter_forms() {
    check_files(&[
        ("Lantern Road c021.cbz", "Lantern Road :: c21"),
        ("Lantern Road C021.cbz", "Lantern Road :: c21"),
        ("Lantern Road ch21.cbz", "Lantern Road :: c21"),
        ("Lantern Road Ch.21.cbz", "Lantern Road :: c21"),
        ("Lantern Road Ch. 21.cbz", "Lantern Road :: c21"),
        ("Lantern Road Ch 21.cbz", "Lantern Road :: c21"),
        ("Lantern Road Ch.21.5.cbz", "Lantern Road :: c21.5"),
        ("Lantern Road Ch 21.5.cbz", "Lantern Road :: c21.5"),
        ("Lantern Road Chapter 21.cbz", "Lantern Road :: c21"),
        ("Lantern Road Chapter 021.cbz", "Lantern Road :: c21"),
        ("Lantern Road - Chapter 21.cbz", "Lantern Road :: c21"),
        ("Lantern Road Chap 21.cbz", "Lantern Road :: c21"),
        ("Lantern Road #021.cbz", "Lantern Road :: c21"),
        ("Lantern Road #21.5.cbz", "Lantern Road :: c21.5"),
        ("Lantern Road - 021.cbz", "Lantern Road :: c21"),
        ("Lantern Road - 21.5.cbz", "Lantern Road :: c21.5"),
        ("Lantern Road c021.5.cbz", "Lantern Road :: c21.5"),
        ("Lantern Road c1000.cbz", "Lantern Road :: c1000"),
        ("Lantern Road c000.cbz", "Lantern Road :: c0"),
        ("Lantern Road 021.cbz", "Lantern Road :: c21"),
        ("Lantern Road c021a.cbz", "Lantern Road :: c21"),
        ("Lantern Road c021v2.cbz", "Lantern Road :: c21"),
    ]);
}

#[test]
fn ranges() {
    check_files(&[
        ("Lantern Road c001-005.cbz", "Lantern Road :: c1-5"),
        ("Lantern Road c001-c005.cbz", "Lantern Road :: c1-5"),
        ("Lantern Road Ch.001-005.cbz", "Lantern Road :: c1-5"),
        ("Lantern Road Chapters 001-005.cbz", "Lantern Road :: c1-5"),
        ("Lantern Road Chapters 1-5.cbz", "Lantern Road :: c1-5"),
        ("Lantern Road c010-012.5.cbz", "Lantern Road :: c10-12.5"),
        ("Lantern Road - 001-005.cbz", "Lantern Road :: c1-5"),
        ("Lantern Road v01 c001-005.cbz", "Lantern Road :: v1 c1-5"),
    ]);
}

#[test]
fn volume_and_chapter_together() {
    check_files(&[
        ("Lantern Road v03 c021.cbz", "Lantern Road :: v3 c21"),
        ("Lantern Road - Vol. 03 Ch. 021.cbz", "Lantern Road :: v3 c21"),
        ("Lantern Road - Volume 03 - Chapter 21.cbz", "Lantern Road :: v3 c21"),
        ("Lantern Road Vol.3 Ch.21.5.cbz", "Lantern Road :: v3 c21.5"),
        ("Lantern Road c021 (v03).cbz", "Lantern Road :: v3 c21"),
        ("Lantern Road - c021 (v03) [Night Owl].cbz", "Lantern Road :: v3 c21 g=Night Owl"),
        ("Lantern Road - c021 [v03].cbz", "Lantern Road :: v3 c21"),
    ]);
}

#[test]
fn chapter_titles() {
    check_files(&[
        ("Lantern Road - Chapter 21 - The Fight.cbz", "Lantern Road :: c21 \"The Fight\""),
        ("Lantern Road - Ch. 021 - The Fight.cbz", "Lantern Road :: c21 \"The Fight\""),
        ("Lantern Road - Vol. 03 Ch. 021 - The Fight [Night Owl].cbz", "Lantern Road :: v3 c21 \"The Fight\" g=Night Owl"),
        ("Lantern Road - 021 - The Fight.cbz", "Lantern Road :: c21 \"The Fight\""),
        ("Lantern Road c021 - The Fight (v03).cbz", "Lantern Road :: v3 c21 \"The Fight\""),
        ("Chapter 21 - The Fight.cbz", " :: c21 \"The Fight\""),
        ("Ch.021 - The Fight.cbz", " :: c21 \"The Fight\""),
    ]);
}

#[test]
fn groups_languages_years_and_extras() {
    check_files(&[
        ("[Night Owl] Lantern Road - c021.cbz", "Lantern Road :: c21 g=Night Owl"),
        ("Lantern Road - c021 [Night Owl].cbz", "Lantern Road :: c21 g=Night Owl"),
        ("Lantern Road - c021 [en].cbz", "Lantern Road :: c21 l=en"),
        ("Lantern Road - c021 [English].cbz", "Lantern Road :: c21 l=en"),
        ("Lantern Road - c021 (fr).cbz", "Lantern Road :: c21 l=fr"),
        ("Lantern Road - c021 [pt-BR].cbz", "Lantern Road :: c21 l=pt-br"),
        ("Lantern Road - c021 [ja] [Night Owl].cbz", "Lantern Road :: c21 g=Night Owl l=ja"),
        ("Lantern Road v03 (2019).cbz", "Lantern Road :: v3 y=2019"),
        ("Lantern Road (2019) v03.cbz", "Lantern Road :: v3 y=2019"),
        ("Lantern Road [2019] c021.cbz", "Lantern Road :: c21 y=2019"),
        ("Lantern Road v03 (2019) (Digital) (1r0n).cbz", "Lantern Road :: v3 y=2019 x=Digital,1r0n"),
        ("Lantern Road v03 (2019) (Digital-HD) (Night Owl-Empire).cbz", "Lantern Road :: v3 y=2019 x=Digital-HD,Night Owl-Empire"),
        ("Lantern Road v03 (Digital).cbz", "Lantern Road :: v3 x=Digital"),
        ("Lantern Road v03 [Digital].cbz", "Lantern Road :: v3 x=Digital"),
        ("Lantern Road v03 (Digital)(1r0n).cbz", "Lantern Road :: v3 x=Digital,1r0n"),
        ("Lantern Road v03 (Fan Colored) [Night Owl].cbz", "Lantern Road :: v3 g=Night Owl x=Fan Colored"),
        ("Lantern Road v03 {Premium}.cbz", "Lantern Road :: v3 x=Premium"),
        ("Lantern Road - c021 [Night Owl] [Raws].cbz", "Lantern Road :: c21 g=Night Owl x=Raws"),
        ("Lantern Road - c021 [Group (Sub)].cbz", "Lantern Road :: c21 g=Group (Sub)"),
    ]);
}

#[test]
fn one_shots() {
    check_files(&[
        ("Lantern Road [Oneshot].cbz", "Lantern Road :: one"),
        ("Lantern Road (Oneshot).cbz", "Lantern Road :: one"),
        ("Lantern Road - Oneshot.cbz", "Lantern Road :: one"),
        ("Lantern Road Oneshot (Digital).cbz", "Lantern Road :: x=Digital one"),
        ("Lantern Road - One-Shot [Night Owl].cbz", "Lantern Road :: g=Night Owl one"),
        ("Lantern Road - c000 [Oneshot].cbz", "Lantern Road :: c0 one"),
    ]);
}

#[test]
fn dotted_and_underscored_names() {
    check_files(&[
        ("Lantern.Road.c021.cbz", "Lantern Road :: c21"),
        ("Lantern.Road.v03.c021.5.cbz", "Lantern Road :: v3 c21.5"),
        ("Lantern_Road_v03_c021.cbz", "Lantern Road :: v3 c21"),
        ("Lantern_Road_-_Ch.21.cbz", "Lantern Road :: c21"),
        ("Lantern.Road.Ch.21.cbz", "Lantern Road :: c21"),
        ("Dr. Stone - c001.cbz", "Dr. Stone :: c1"),
        ("Dr. Stone v01.cbz", "Dr. Stone :: v1"),
    ]);
}

#[test]
fn titles_that_look_like_markers() {
    check_files(&[
        ("Vampire Knight v01.cbz", "Vampire Knight :: v1"),
        ("Voyage c03.cbz", "Voyage :: c3"),
        ("Cells at Work c012.cbz", "Cells at Work :: c12"),
        ("Chainsaw Days c012.cbz", "Chainsaw Days :: c12"),
        ("20th Century Lanterns v03 c021.cbz", "20th Century Lanterns :: v3 c21"),
        ("Mob Psycho 100 c021.cbz", "Mob Psycho 100 :: c21"),
        ("Mob Psycho 100 - 021.cbz", "Mob Psycho 100 :: c21"),
        ("Mob Psycho 100.cbz", "Mob Psycho 100 :: "),
        ("Blade Runner 2049 c001.cbz", "Blade Runner 2049 :: c1"),
        ("3-gatsu Lanterns c001.cbz", "3-gatsu Lanterns :: c1"),
        ("Re:Lanterns - c001.cbz", "Re:Lanterns :: c1"),
        ("K-On! v02.cbz", "K-On! :: v2"),
        ("Lantern Road.cbz", "Lantern Road :: "),
    ]);
}

#[test]
fn extensions() {
    for ext in ["cbz", "cbr", "cb7", "cbt", "zip", "rar", "7z", "pdf", "epub", "CBZ", "Cbz"] {
        let p = parse_file_name(&format!("Lantern Road v03 c021.{ext}"));
        assert_eq!(show(&p), "Lantern Road :: v3 c21", "{ext}");
    }
    // Not a comic extension: nothing is cut (the dot belongs to the number).
    assert_eq!(show(&parse_file_name("Lantern Road Ch.21")), "Lantern Road :: c21");
    assert_eq!(show(&parse_file_name("Lantern Road Ch.21.5")), "Lantern Road :: c21.5");
    // A folder or a name with no extension.
    assert_eq!(show(&parse_file_name("Lantern Road (2019)")), "Lantern Road :: y=2019");
}

#[test]
fn mangadex_and_digital_release_styles() {
    check_files(&[
        // MangaDex's downloader and common mirrors.
        ("Lantern Road - Vol. 03 Ch. 021 - The Fight [en] [Night Owl].cbz", "Lantern Road :: v3 c21 \"The Fight\" g=Night Owl l=en"),
        ("Lantern Road - Ch. 021 [Night Owl].cbz", "Lantern Road :: c21 g=Night Owl"),
        ("Lantern Road - Vol.03 Ch.021 (en) [Night Owl].cbz", "Lantern Road :: v3 c21 g=Night Owl l=en"),
        ("Lantern Road - c021 (v03) [Night Owl].cbz", "Lantern Road :: v3 c21 g=Night Owl"),
        ("Lantern Road - c021-025 (v03) [Night Owl].cbz", "Lantern Road :: v3 c21-25 g=Night Owl"),
        ("Lantern Road - Oneshot [Night Owl].cbz", "Lantern Road :: g=Night Owl one"),
        // Digital volume rips (the scene style).
        ("Lantern Road v01 (2018) (Digital) (1r0n).cbz", "Lantern Road :: v1 y=2018 x=Digital,1r0n"),
        ("Lantern Road v12 (2021) (Digital) (Night Owl-Empire).cbz", "Lantern Road :: v12 y=2021 x=Digital,Night Owl-Empire"),
        ("Lantern Road - Volume 05 (2020) (Digital).cbz", "Lantern Road :: v5 y=2020 x=Digital"),
        ("Lantern Road, Vol. 05 (2020) (Digital).cbz", "Lantern Road :: v5 y=2020 x=Digital"),
        ("Lantern Road - Vol. 05 [Digital].cbz", "Lantern Road :: v5 x=Digital"),
        // Fan scanlation style.
        ("[Night Owl] Lantern Road - Ch. 021.5.cbz", "Lantern Road :: c21.5 g=Night Owl"),
        ("[Night Owl] Lantern Road v03 [Digital].cbz", "Lantern Road :: v3 g=Night Owl x=Digital"),
        ("[Night Owl][en] Lantern Road c021.cbz", "Lantern Road :: c21 g=Night Owl l=en"),
    ]);
}

#[test]
fn paths_take_the_series_from_the_folder() {
    check_paths(&[
        ("Lantern Road/c021.cbz", "Lantern Road :: c21"),
        ("Lantern Road/Ch. 21.5.cbz", "Lantern Road :: c21.5"),
        ("Lantern Road/021.cbz", "Lantern Road :: c21"),
        ("Lantern Road/21.cbz", "Lantern Road :: c21"),
        ("Lantern Road/Chapter 21 - The Fight.cbz", "Lantern Road :: c21 \"The Fight\""),
        ("Lantern Road (2019)/c021.cbz", "Lantern Road :: c21 y=2019"),
        ("Lantern Road (2019) [Digital]/c021.cbz", "Lantern Road :: c21 y=2019"),
        ("/Lantern Road/c021.cbz", "Lantern Road :: c21"),
        ("Shelf/Lantern Road/c021.cbz", "Lantern Road :: c21"),
        ("Lantern Road/v03.cbz", "Lantern Road :: v3"),
        ("Lantern Road/001-005.cbz", "Lantern Road :: c1-5"),
    ]);
}

#[test]
fn the_file_name_wins_over_the_folder() {
    check_paths(&[
        ("Some Folder/Lantern Road - c021.cbz", "Lantern Road :: c21"),
        ("Lantern Road/Lantern Road v03 c021.cbz", "Lantern Road :: v3 c21"),
        ("Lantern Road/[Night Owl] Lantern Road c021.cbz", "Lantern Road :: c21 g=Night Owl"),
        ("Lantern Road (2019)/Lantern Road c021 (2020).cbz", "Lantern Road :: c21 y=2020"),
    ]);
}

#[test]
fn yacreader_library_layouts() {
    check_paths(&[
        // Series / Volume folder / chapter file.
        ("Lantern Road/Volume 03/Chapter 021.cbz", "Lantern Road :: v3 c21"),
        ("Lantern Road/Vol. 03/c021.cbz", "Lantern Road :: v3 c21"),
        ("Lantern Road/v03/021.cbz", "Lantern Road :: v3 c21"),
        ("Lantern Road/Volume 03/Ch.21.5 - The Fight.cbz", "Lantern Road :: v3 c21.5 \"The Fight\""),
        // Series / volume files.
        ("Lantern Road/Lantern Road v03 (2019) (Digital) (1r0n).cbz", "Lantern Road :: v3 y=2019 x=Digital,1r0n"),
        ("Lantern Road/Lantern Road - Volume 03.cbz", "Lantern Road :: v3"),
        // One folder per volume, files named by chapter.
        ("Lantern Road v03/c021.cbz", "Lantern Road :: v3 c21"),
        ("Lantern Road Vol. 03/Ch. 21.cbz", "Lantern Road :: v3 c21"),
        // A chapter folder holding the file.
        ("Lantern Road/Chapter 21/Lantern Road - Chapter 21.cbz", "Lantern Road :: c21"),
        ("Lantern Road/Chapter 21/01.cbz", "Lantern Road :: c1"),
        // Genre / series / file (only the nearest folder with a title counts).
        ("Action/Lantern Road/c021.cbz", "Lantern Road :: c21"),
        // A bare number after the title, backed by the folder.
        ("Lantern Road/Lantern Road 21.cbz", "Lantern Road :: c21"),
        ("Mob Psycho 100/Mob Psycho 100 21.cbz", "Mob Psycho 100 :: c21"),
        // No help from the folder: the number stays in the title.
        ("Shelf/Mob Psycho 100.cbz", "Mob Psycho 100 :: "),
        ("Lantern Road/Another Title 21.cbz", "Another Title 21 :: "),
    ]);
}

#[test]
fn empty_and_odd_input() {
    assert_eq!(parse_path(""), Parsed::default());
    assert_eq!(parse_path("/"), Parsed::default());
    assert_eq!(parse_file_name(""), Parsed::default());
    let _ = parse_file_name(".cbz"); // a hidden file: no panic
    // An unclosed bracket is text.
    assert_eq!(show(&parse_file_name("Lantern Road [Night c021.cbz")), "Lantern Road [Night :: c21");
    // Only brackets.
    assert_eq!(show(&parse_file_name("[Night Owl].cbz")), " :: g=Night Owl");
    // A marker word with no number.
    assert_eq!(show(&parse_file_name("Chapter Zero.cbz")), "Chapter Zero :: ");
    // Non-ASCII titles.
    assert_eq!(show(&parse_file_name("ランタン通り v03 c021 [Night Owl].cbz")), "ランタン通り :: v3 c21 g=Night Owl");
}

#[test]
fn spans() {
    let p = parse_file_name("Lantern Road c010-012.cbz");
    let c = p.chapter.unwrap();
    assert!(c.is_range());
    assert!(c.contains(11.0));
    assert!(!c.contains(13.0));
    assert_eq!(p.last_chapter(), Some(12.0));
    assert_eq!(parse_file_name("Lantern Road c021.5.cbz").last_chapter(), Some(21.5));
    assert_eq!(parse_file_name("Lantern Road v03.cbz").last_chapter(), None);
}

#[test]
fn japanese_markers() {
    check_files(&[
        ("Lantern Road 第21話.cbz", "Lantern Road :: c21"),
        ("Lantern Road 第3巻.cbz", "Lantern Road :: v3"),
        ("Lantern Road 第03巻 第021話.cbz", "Lantern Road :: v3 c21"),
        ("ランタン通り第3巻.cbz", "ランタン通り :: v3"),
        ("Lantern Road Tome 3.cbz", "Lantern Road :: v3"),
    ]);
}

/// What the parser does not do (yet). If one of these changes, change the doc comment too.
#[test]
fn known_limits() {
    check_files(&[
        // A bare number without a leading zero stays in the title: it could be part of it. The
        // parent folder can settle it (`Berserk/Berserk 364.cbz`, see `parse_path`).
        ("Berserk 364.cbz", "Berserk 364 :: "),
        ("Lantern Road 21.1.cbz", "Lantern Road 21.1 :: "),
        ("Lantern Road 3.epub", "Lantern Road 3 :: "),
        // Words that are not markers here.
        ("Lantern Road Ep. 12.cbz", "Lantern Road Ep. 12 :: "),
        ("Lantern Road Issue 12.cbz", "Lantern Road Issue 12 :: "),
        ("Lantern Road T03.cbz", "Lantern Road T03 :: "),
        // Only the first volume and the first chapter count.
        ("Lantern Road v03 c021 c022.cbz", "Lantern Road :: v3 c21 \"c022\""),
        // Part numbers are the chapter's title, not a number.
        ("Lantern Road Chapter 21 Part 2.cbz", "Lantern Road :: c21 \"Part 2\""),
        // A leading-zero bare number is a chapter, even when it is "of" something.
        ("Lantern Road 03 (of 12).cbz", "Lantern Road :: c3 x=of 12"),
        // Date ranges are extras, not a year.
        ("Lantern Road (2019-2021) v03.cbz", "Lantern Road :: v3 x=2019-2021"),
    ]);
}
