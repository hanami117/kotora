import Foundation

// The list page's power features (desktop `model.rs`/`manga.rs`): the four orders and their two
// directions, the chips, the "Filter your list" box, the view per tab, "Ep N out" / "On hiatus",
// the score in the user's format and the empty states. The core computes it all
// (`mobile/src/listopts.rs`); this file is its Swift side and the sample data's copy.

/// What a repository gives the list page beyond the plain list. Separate from `Repository` so
/// that the protocol everyone edits stays as it is.
protocol ListPageSource: AnyObject {
    func listPage(kind: MediaKind, query: ListQuery) async throws -> ListPage
    func listView(kind: MediaKind, status: ListStatus) async -> ListViewMode
    func setListView(kind: MediaKind, status: ListStatus, view: ListViewMode) async
}

extension CoreRepository: ListPageSource {
    func listPage(kind: MediaKind, query: ListQuery) async throws -> ListPage {
        let k = kotora
        return try await Task.detached(priority: .userInitiated) { try k.listPage(kind: kind, q: query) }.value
    }

    func listView(kind: MediaKind, status: ListStatus) async -> ListViewMode {
        let k = kotora
        return await Task.detached(priority: .userInitiated) { k.listView(kind: kind, status: status) }.value
    }

    func setListView(kind: MediaKind, status: ListStatus, view: ListViewMode) async {
        let k = kotora
        await Task.detached(priority: .userInitiated) { try? k.setListView(kind: kind, status: status, view: view) }.value
    }
}

extension ListSort: CaseIterable {
    public static var allCases: [ListSort] { listSorts() }
    var label: String { listSortLabel(sort: self) }
    var directions: [String] { listSortDirections(sort: self) }
}

extension QuickFilter {
    var label: String { quickFilterLabel(filter: self) }
}

extension ListViewMode: CaseIterable {
    public static var allCases: [ListViewMode] { [.list, .grid, .large] }
    var label: String { listViewLabel(view: self) }
    var icon: String {
        switch self {
        case .list: return "list.bullet"
        case .grid: return "square.grid.3x3"
        case .large: return "square.grid.2x2"
        }
    }
}

extension PageLine {
    /// The tags under a title, with "Ep N out" and "On hiatus" (desktop `grid.rs`, `mgrid.rs`).
    var tags: [(String, TagStyle)] {
        var out = row.tags
        if let n = newEpisode { out.insert(("Ep \(n) out", .new), at: min(out.count, row.rewatching ? 1 : 0)) }
        if onHiatus && !row.rewatching { out.insert(("On hiatus", .hiatus), at: 0) }
        return Array(out.prefix(3))
    }
}

// ------------------------------------------------------------------------------- sample data

private var sampleViews: [String: ListViewMode] = [:]
private let sampleLock = NSLock()

extension SampleRepository: ListPageSource {
    func listView(kind: MediaKind, status: ListStatus) async -> ListViewMode {
        sampleLock.withLock { sampleViews["\(kind)\(status)"] ?? .list }
    }

    func setListView(kind: MediaKind, status: ListStatus, view: ListViewMode) async {
        sampleLock.withLock { sampleViews["\(kind)\(status)"] = view }
    }

    /// The core's rules on the made-up rows (`listopts.rs`).
    func listPage(kind: MediaKind, query q: ListQuery) async throws -> ListPage {
        let all = locked { rows.filter { $0.kind == kind } }
        let needle = q.query.trimmingCharacters(in: .whitespaces).lowercased()
        let newEp: (Row) -> UInt32? = { r in
            // Made up: the airing shows being watched have one more episode out.
            guard r.kind == .anime, r.airStatus == "currently_airing", r.status == .watching, r.id % 2 == 1 else { return nil }
            return r.progress + 1
        }
        func keeps(_ r: Row) -> Bool {
            switch q.filter {
            case .all: return true
            case .airing: return r.airStatus == "currently_airing"
            case .finishedAiring: return r.airStatus == "finished_airing"
            case .newEpisode: return newEp(r) != nil
            case .publishing: return r.airStatus == "currently_publishing"
            case .finished: return r.airStatus == "finished"
            case .onHiatus: return r.airStatus == "on_hiatus"
            }
        }
        let onPage = { (r: Row) in Self.showsIn(r, q.status) }
        var l = all.filter { r in
            needle.isEmpty ? onPage(r) && keeps(r)
                : ([r.title, r.titleEn].compactMap { $0?.lowercased() }.contains { $0.contains(needle) })
        }
        let score = { (r: Row) in Float(r.score ?? "") ?? -1 }
        switch q.sort {
        case .updated: l.sort { $0.updatedAt > $1.updatedAt }
        case .title: l.sort { $0.title.lowercased() < $1.title.lowercased() }
        case .score: l.sort { score($0) > score($1) }
        case .progress: l.sort { ($0.progress, $0.volumes) > ($1.progress, $1.volumes) }
        }
        if q.reverse { l.reverse() }
        if !needle.isEmpty {
            let rank = { (r: Row) in r.status == q.status ? 0 : 1 + (ListStatus.allCases.firstIndex(of: r.status) ?? 0) }
            l = l.enumerated().sorted { (rank($0.element), $0.offset) < (rank($1.element), $1.offset) }.map(\.element)
        }
        let lines = l.map { r in
            PageLine(row: r, newEpisode: newEp(r), onHiatus: r.airStatus == "on_hiatus",
                     scoreMark: r.score.map { ScoreMark(style: .number, text: $0, stars: 0, face: nil, words: $0) })
        }
        if !lines.isEmpty { return ListPage(lines: lines, empty: .none, emptyTitle: "", emptyHelp: "") }
        if needle.isEmpty && !all.contains(where: onPage) {
            let noun = kind == .manga ? "manga" : "anime"
            return ListPage(lines: [], empty: .empty, emptyTitle: "No \(noun) in \(q.status.label(kind))",
                            emptyHelp: "Entries you drop on MyAnimeList show up here.")
        }
        let t = needle.isEmpty ? "No \(kind == .manga ? "manga" : "anime") match this filter."
            : "Nothing matches \u{201c}\(q.query.trimmingCharacters(in: .whitespaces))\u{201d}."
        return ListPage(lines: [], empty: .noMatch, emptyTitle: t, emptyHelp: "")
    }
}
