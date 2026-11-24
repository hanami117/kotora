import Foundation

// The words the screens show, the same as the Android app's (ui/Components.kt, ListScreen.kt).

extension ListStatus: CaseIterable {
    public static var allCases: [ListStatus] { [.watching, .planToWatch, .completed, .onHold, .dropped] }

    func label(_ kind: MediaKind) -> String {
        switch self {
        case .watching: return kind == .manga ? "Reading" : "Watching"
        case .planToWatch: return kind == .manga ? "Plan to read" : "Plan to watch"
        case .completed: return "Completed"
        case .onHold: return "On hold"
        case .dropped: return "Dropped"
        }
    }
}

extension Service {
    var label: String { self == .mal ? "MyAnimeList" : "AniList" }
}

extension MediaKind {
    var unit: String { self == .manga ? "chapter" : "episode" }
}

enum TagStyle { case air, wait, plain, new, hiatus }

extension Row {
    var key: String { "\(kind == .manga ? "m" : "a")\(id)" }

    var initials: String {
        let words = title.split(whereSeparator: { $0 == " " || $0 == "-" || $0 == ":" })
            .filter { $0.first.map { $0.isLetter || $0.isNumber } == true }
        return String(words.prefix(2).compactMap(\.first)).uppercased()
    }

    var formatLabel: String {
        switch format.lowercased() {
        case "tv": return "TV"
        case "tv_special", "special": return "Special"
        case "movie": return "Movie"
        case "ova": return "OVA"
        case "ona": return "ONA"
        case "music": return "Music"
        case "manga": return "Manga"
        case "light_novel", "novel": return "Light novel"
        case "one_shot": return "One-shot"
        case "manhwa": return "Manhwa"
        case "manhua": return "Manhua"
        case "": return ""
        default: return format.replacingOccurrences(of: "_", with: " ").capitalized
        }
    }

    /// The waiting change's tag: "Waiting to sync", "Conflict in queue", "Sync failed" (desktop `waiting_label`).
    var waitingText: String { syncLabel ?? "Waiting to sync" }
    /// A conflict or a failure: the tag is red.
    var waitingProblem: Bool { syncLabel != nil && syncLabel != "Waiting to sync" }

    /// A manga read by volume (no chapter yet, some volumes): +1 steps volumes, as on the desktop.
    var plusVolumes: Bool { kind == .manga && progress == 0 && volumes > 0 }

    /// "One more episode", "One more chapter", "One more volume" (the core's words).
    var plusOneText: String { plusOneLabel(row: self) }

    /// +1 is offered while watching (or rewatching) and before the last episode (volume).
    var canPlusOne: Bool {
        let atEnd = plusVolumes ? totalVolumes.map { volumes >= $0 } : total.map { progress >= $0 }
        return (status == .watching || rewatching) && !(atEnd ?? false)
    }

    /// The small tags under a title: rewatch, airing, waiting, format. At most three.
    var tags: [(String, TagStyle)] {
        var out: [(String, TagStyle)] = []
        if rewatching { out.append((kind == .manga ? "Rereading" : "Rewatching", .plain)) }
        switch airStatus {
        case "currently_airing", "RELEASING": out.append(("Airing", .air))
        case "currently_publishing": out.append(("Publishing", .air))
        case "not_yet_aired", "NOT_YET_RELEASED", "not_yet_published": out.append(("Not aired yet", .plain))
        default: break
        }
        if waiting { out.append((waitingText, waitingProblem ? .hiatus : .wait)) }
        if out.count < 2 {
            let f = formatLabel
            if !f.isEmpty {
                out.append((total.map { kind == .anime && f == "TV" ? "\(f) · \($0) eps" : f } ?? f, .plain))
            }
        }
        return Array(out.prefix(3))
    }

    /// The pill on the details page.
    var airPill: (String, Bool)? {
        switch airStatus {
        case "currently_airing", "RELEASING": return ("Airing", true)
        case "currently_publishing": return ("Publishing", true)
        case "finished_airing", "FINISHED", "finished": return ("Finished", false)
        case "not_yet_aired", "NOT_YET_RELEASED", "not_yet_published": return ("Not aired yet", false)
        default: return nil
        }
    }

    /// "TV · 12 episodes · Fall 2026".
    var meta: String {
        var parts: [String] = []
        if !formatLabel.isEmpty { parts.append(formatLabel) }
        if let n = total { parts.append("\(n) \(kind == .manga ? "chapters" : n == 1 ? "episode" : "episodes")") }
        if let d = startDate, let s = season(d) { parts.append(s) }
        return parts.joined(separator: " · ")
    }
}

/// `2026-10-03` → "Fall 2026" (anime seasons by month).
func season(_ date: String) -> String? {
    guard let y = Int(date.prefix(4)) else { return nil }
    let parts = date.split(separator: "-")
    guard parts.count > 1, let m = Int(parts[1]) else { return String(y) }
    let s = m <= 3 ? "Winter" : m <= 6 ? "Spring" : m <= 9 ? "Summer" : "Fall"
    return "\(s) \(y)"
}

/// "Synced 4 min ago".
func ago(_ then: Int64, _ now: Int64) -> String {
    let s = max(0, now - then)
    if s < 60 { return "just now" }
    if s < 3600 { return "\(s / 60) min ago" }
    if s < 86_400 { return "\(s / 3600) h ago" }
    return "\(s / 86_400) d ago"
}

func nowSeconds() -> Int64 { Int64(Date().timeIntervalSince1970) }
