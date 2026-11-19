import Foundation

/// Search and add (the desktop's search page): the active service's results, and an add that goes
/// into the core's queue. Its own protocol, next to `Repository`, so the two repositories gain it
/// in this file.
protocol SearchRepository: AnyObject, Sendable {
    /// Network.
    func search(kind: MediaKind, query: String) async throws -> [Hit]
    func add(_ hit: Hit, status: ListStatus) async throws
}

extension CoreRepository: SearchRepository {
    func search(kind: MediaKind, query: String) async throws -> [Hit] {
        try await Task.detached(priority: .userInitiated) { try self.kotora.search(kind: kind, query: query) }.value
    }

    func add(_ hit: Hit, status: ListStatus) async throws {
        try await Task.detached(priority: .userInitiated) { try self.kotora.add(hit: hit, status: status) }.value
    }
}

extension SampleRepository: SearchRepository {
    /// Made-up results (the desktop's scene titles), a short pause as if the service answered.
    func search(kind: MediaKind, query: String) async throws -> [Hit] {
        try? await Task.sleep(for: .milliseconds(350))
        let q = query.lowercased()
        return Self.sampleHits(kind).filter { h in
            q.split(separator: " ").allSatisfy { w in ([h.title, h.titleEn ?? ""] + h.synonyms).contains { $0.lowercased().contains(w) } }
                || "lanterns".hasPrefix(q) || q.contains("lantern")
        }
    }

    func add(_ hit: Hit, status: ListStatus) async throws {
        locked {
            if let i = rows.firstIndex(where: { $0.kind == hit.kind && $0.id == hit.id }) {
                rows[i].status = status
                rows[i].waiting = true
                rows[i].updatedAt = Int64(Date().timeIntervalSince1970)
                return
            }
            let done = status == .completed
            rows.append(Row(kind: hit.kind, id: hit.id, title: hit.title, titleEn: hit.titleEn, status: status,
                            progress: done ? hit.episodes ?? 0 : 0, total: hit.episodes, volumes: done ? hit.volumes ?? 0 : 0,
                            totalVolumes: hit.volumes, score: nil, format: hit.format, airStatus: hit.airStatus,
                            startDate: hit.startDate, coverUrl: hit.coverUrl, rewatching: false, timesRewatched: 0,
                            waiting: true, updatedAt: Int64(Date().timeIntervalSince1970)))
        }
    }

    static func sampleHits(_ kind: MediaKind) -> [Hit] {
        func h(_ id: UInt32, _ title: String, _ en: String?, _ format: String, _ eps: UInt32?, _ air: String, _ type: String, _ meta: String) -> Hit {
            Hit(kind: kind, id: id, title: title, titleEn: en, titleJa: nil, synonyms: [], format: format, episodes: eps, volumes: nil,
                airStatus: air, seasonYear: nil, seasonName: nil, mean: nil, coverUrl: nil, authors: [], startDate: nil,
                typeLabel: type, meta: meta)
        }
        if kind == .manga {
            return [
                h(9201, "Lanterns of Kirigaoka", nil, "manga", 48, "currently_publishing", "Manga", "48 ch · 6 vols · Publishing · ★ 8.12"),
                h(9202, "Lanterns of Kirigaoka: Night Market", nil, "light_novel", nil, "finished", "Light novel", "Finished · ★ 7.40"),
                h(101, "Paper Lanterns", nil, "manga", nil, "currently_publishing", "Manga", "Publishing · ★ 8.01"),
            ]
        }
        return [
            h(9101, "Lanterns of Kirigaoka: Festival Night", "Lanterns: Festival Night", "movie", 1, "finished_airing", "Movie", "1 ep · Summer 2026 · ★ 8.31"),
            h(9102, "Lanterns of Kirigaoka 2nd Season", nil, "tv", 12, "currently_airing", "TV", "12 eps · Airing · Fall 2026 · ★ 8.05"),
            h(9103, "Kirigaoka Lantern Shop", "The Lantern Shop on the Hill", "ona", 24, "finished_airing", "ONA", "24 eps · Spring 2023 · ★ 7.12"),
            h(9104, "Lanterns of Kirigaoka: Side Stories", nil, "special", 6, "not_yet_aired", "Special", "6 eps · Not yet aired"),
            h(19, "Ashita no Lantern", nil, "tv", 25, "finished_airing", "TV", "25 eps · Spring 2019 · ★ 8.44"),
        ]
    }
}
