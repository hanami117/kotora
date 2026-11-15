# Bundled data

- `anime-relations.txt`: episode redirection rules from
  [erengy/anime-relations](https://github.com/erengy/anime-relations), made for Taiga by its author and
  contributors. It is in the public domain (CC0). Copied unchanged on 2026-11-15 (`last_modified:
  2026-10-19`). The app downloads a newer copy at most once a week (see `core/src/recognize/relations.rs`).
  To update the built-in copy, replace the file and run `cargo test -p kotora-core relations`.
