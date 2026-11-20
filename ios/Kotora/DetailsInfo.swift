import SwiftUI

// The details page's service information, as the desktop's (`app/src/details.rs`): the stats
// row, the other titles and synonyms, Synopsis and Information, the loading and offline states,
// and the "Your list" card of a title that is not on the list, with its add buttons.

/// The service's details and the add of a title not on the list. Its own protocol, next to
/// `Repository`, so the two repositories gain it in this file.
protocol DetailsRepository: AnyObject, Sendable {
    /// Local: the saved copy, shown at once.
    func savedDetails(kind: MediaKind, id: UInt32) async -> ServiceDetails?
    /// Network.
    func fetchDetails(kind: MediaKind, id: UInt32) async throws -> ServiceDetails
    func addTitle(_ stub: TitleStub, status: ListStatus) async throws
    /// A pasted MyAnimeList/AniList link (of the active service) or a bare id.
    func pastedId(kind: MediaKind, text: String) -> PastedId?
}

extension CoreRepository: DetailsRepository {
    func savedDetails(kind: MediaKind, id: UInt32) async -> ServiceDetails? {
        await Task.detached(priority: .userInitiated) { self.kotora.savedDetails(kind: kind, id: id) }.value
    }

    func fetchDetails(kind: MediaKind, id: UInt32) async throws -> ServiceDetails {
        try await Task.detached(priority: .userInitiated) { try self.kotora.fetchDetails(kind: kind, id: id) }.value
    }

    func addTitle(_ stub: TitleStub, status: ListStatus) async throws {
        try await Task.detached(priority: .userInitiated) { try self.kotora.addTitle(stub: stub, status: status) }.value
    }

    func pastedId(kind: MediaKind, text: String) -> PastedId? {
        kotora.pastedId(kind: kind, text: text)
    }
}

extension SampleRepository: DetailsRepository {
    func savedDetails(kind: MediaKind, id: UInt32) async -> ServiceDetails? { nil }

    /// Made-up information, after a short pause as if the service answered.
    func fetchDetails(kind: MediaKind, id: UInt32) async throws -> ServiceDetails {
        try? await Task.sleep(for: .milliseconds(400))
        let row = locked { rows.first { $0.kind == kind && $0.id == id } }
        let hit = SampleRepository.sampleHits(kind).first { $0.id == id }
        let title = row?.title ?? hit?.title ?? "Lanterns of Kirigaoka"
        let manga = kind == .manga
        let facts: [InfoFact] = manga
            ? [InfoFact(label: "Published", value: "Since Apr 4, 2021"), InfoFact(label: "Authors", value: "Aoi Mori (Story & Art)"),
               InfoFact(label: "Serialization", value: "Monthly Lantern"), InfoFact(label: "Chapters", value: "Unknown while publishing"),
               InfoFact(label: "Volumes", value: "Unknown while publishing")]
            : [InfoFact(label: "Aired", value: "Since Oct 3, 2026"), InfoFact(label: "Studios", value: "Studio Hoshi"),
               InfoFact(label: "Source", value: "Light novel"), InfoFact(label: "Length", value: "24 min per episode")]
        return ServiceDetails(
            kind: kind, id: id, service: .mal, title: title, titleEn: row?.titleEn ?? hit?.titleEn, titleJa: "霧ヶ丘の提灯",
            otherTitles: [row?.titleEn ?? hit?.titleEn, "霧ヶ丘の提灯"].compactMap { $0 }.filter { $0 != title },
            also: "Also: Kirigaoka Lanterns", format: manga ? "manga" : "tv",
            tags: manga ? ["Manga", "Publishing"] : ["TV", "12 episodes", "Airing", "Fall 2026"], running: true,
            episodes: manga ? nil : 12, volumes: nil, airStatus: manga ? "currently_publishing" : "currently_airing", mean: 8.31,
            stats: [InfoStat(value: "8.31", label: "MAL", star: true), InfoStat(value: "#412", label: "ranked", star: false),
                    InfoStat(value: "#1,208", label: "popular", star: false), InfoStat(value: "182,340", label: "members", star: false)],
            synopsis: "Every autumn the hillside town of Kirigaoka hangs a thousand paper lanterns for the festival of returning lights. This year Hana, the lantern maker's daughter, finds one that glows on its own.",
            facts: facts, genres: ["Drama", "Slice of Life", "Supernatural"], coverUrl: row?.coverUrl ?? hit?.coverUrl, coverSmall: nil,
            fetchedAt: Int64(Date().timeIntervalSince1970), fresh: true, onList: row != nil)
    }

    func addTitle(_ stub: TitleStub, status: ListStatus) async throws {
        let hit = Hit(kind: stub.kind, id: stub.id, title: stub.title, titleEn: nil, titleJa: nil, synonyms: [], format: stub.format,
                      episodes: stub.episodes, volumes: nil, airStatus: "", seasonYear: nil, seasonName: nil, mean: nil,
                      coverUrl: stub.coverUrl, authors: [], startDate: nil, typeLabel: "", meta: "")
        try await add(hit, status: status)
    }

    func pastedId(kind: MediaKind, text: String) -> PastedId? {
        let t = text.trimmingCharacters(in: .whitespaces)
        if let n = UInt32(t), n > 0, t.count <= 9 { return PastedId(id: n, link: false) }
        let digits = t.components(separatedBy: "/\(kind == .manga ? "manga" : "anime")/").dropFirst().first?.prefix { $0.isNumber }
        guard t.contains("myanimelist.net"), let d = digits, let n = UInt32(d), n > 0 else { return nil }
        return PastedId(id: n, link: true)
    }
}

/// The page's service information: the saved copy at once, then the service's answer.
@MainActor
final class DetailsInfo: ObservableObject {
    @Published private(set) var info: ServiceDetails?
    @Published private(set) var loading = false
    @Published private(set) var error: String?

    func load(_ model: AppModel, kind: MediaKind, id: UInt32) async {
        guard let repo = model.repo as? DetailsRepository else { return }
        error = nil
        if info == nil, let saved = await repo.savedDetails(kind: kind, id: id) {
            info = saved
            if saved.fresh { return }
        }
        loading = true
        do {
            info = try await repo.fetchDetails(kind: kind, id: id)
            error = nil
        } catch {
            // Saved information already shows: a failed refresh only ends the loading.
            if info == nil { self.error = message(of: error) }
        }
        loading = false
    }
}

/// Score, rank, popularity, members.
struct DetailsStats: View {
    @Environment(\.palette) private var p
    let info: ServiceDetails

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(info.stats, id: \.label) { s in
                    VStack(alignment: .leading, spacing: 2) {
                        HStack(spacing: 4) {
                            if s.star { Image(systemName: "star.fill").font(.system(size: 12)).foregroundStyle(p.accent) }
                            Text(s.value).font(Inter.semibold(16)).monospacedDigit().foregroundStyle(p.text)
                        }
                        Text(s.label).font(Inter.regular(11.5)).foregroundStyle(p.faint)
                    }
                    .padding(.horizontal, 12).padding(.vertical, 8)
                    .background(RoundedRectangle(cornerRadius: 10).fill(p.surface))
                    .overlay(RoundedRectangle(cornerRadius: 10).stroke(p.border, lineWidth: 1))
                    .accessibilityElement(children: .combine)
                }
            }
            .padding(.horizontal, 16)
        }
        .accessibilityIdentifier("details/stats")
    }
}

/// The other titles (English, Japanese) and "Also: synonyms".
struct DetailsTitles: View {
    @Environment(\.palette) private var p
    let info: ServiceDetails
    /// The title the header shows already.
    let shown: [String]

    var body: some View {
        let others = info.otherTitles.filter { !shown.contains($0) }
        if !others.isEmpty || info.also != nil {
            VStack(alignment: .leading, spacing: 3) {
                ForEach(others, id: \.self) { Text($0).font(Inter.medium(13.5)).foregroundStyle(p.dim).textSelection(.enabled) }
                if let also = info.also { Text(also).font(Inter.regular(12.5)).foregroundStyle(p.dim).textSelection(.enabled) }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 16).padding(.bottom, 12)
        }
    }
}

/// Synopsis and Information, or the error with Try again, or the skeleton while loading.
struct DetailsBody: View {
    @Environment(\.palette) private var p
    @ObservedObject var state: DetailsInfo
    let service: String
    let retry: () -> Void
    @State private var more = false

    var body: some View {
        VStack(spacing: 16) {
            if let i = state.info {
                if state.loading {
                    Text("Updating…").font(Inter.regular(12)).foregroundStyle(p.faint)
                        .frame(maxWidth: .infinity, alignment: .leading).padding(.horizontal, 16)
                }
                Card {
                    title("Synopsis")
                    Text(i.synopsis).font(Inter.regular(14)).foregroundStyle(p.dim).lineSpacing(3)
                        .lineLimit(more ? nil : 6).textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.horizontal, 16).padding(.bottom, i.synopsis.count > 320 ? 6 : 14)
                        .accessibilityIdentifier("details/synopsis")
                    if i.synopsis.count > 320 {
                        Button(more ? "Less" : "More") { withAnimation(.easeOut(duration: 0.2)) { more.toggle() } }
                            .font(Inter.medium(13)).foregroundStyle(p.accentHi).buttonStyle(.borderless)
                            .frame(maxWidth: .infinity, alignment: .leading).padding(.horizontal, 16).padding(.bottom, 12)
                    }
                }
                Card {
                    title("Information")
                    VStack(alignment: .leading, spacing: 10) {
                        ForEach(Array(i.facts.enumerated()), id: \.offset) { n, f in
                            if n == 1 || (i.kind == .manga && f.label == "Chapters") { genres(i) }
                            fact(f.label) { Text(f.value).font(Inter.regular(14)).foregroundStyle(p.text).textSelection(.enabled) }
                        }
                        if i.facts.count < 2 { genres(i) }
                    }
                    .padding(.horizontal, 16).padding(.bottom, 14)
                    .accessibilityIdentifier("details/facts")
                }
            } else if let e = state.error {
                HStack(spacing: 12) {
                    Text(e).font(Inter.regular(14)).foregroundStyle(p.text).frame(maxWidth: .infinity, alignment: .leading)
                    Button("Try again", action: retry).font(Inter.medium(14)).foregroundStyle(p.accent).buttonStyle(.borderless)
                        .accessibilityIdentifier("details/retry")
                }
                .padding(.horizontal, 14).padding(.vertical, 10)
                .background(RoundedRectangle(cornerRadius: 10).fill(p.dropped.opacity(0.08)))
                .overlay(RoundedRectangle(cornerRadius: 10).stroke(p.dropped.opacity(0.3), lineWidth: 1))
                .padding(.horizontal, 16)
            } else {
                VStack(alignment: .leading, spacing: 8) {
                    Text("Loading from \(service)…").font(Inter.regular(12)).foregroundStyle(p.faint)
                    ForEach(0..<3, id: \.self) { n in
                        RoundedRectangle(cornerRadius: 10).fill(p.raised).frame(height: n == 2 ? 90 : 36)
                    }
                }
                .padding(.horizontal, 16)
                .accessibilityIdentifier("details/loading")
            }
        }
    }

    private func title(_ t: String) -> some View {
        Text(t).font(Inter.semibold(15)).foregroundStyle(p.text)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 16).padding(.top, 14).padding(.bottom, 8)
    }

    @ViewBuilder private func genres(_ i: ServiceDetails) -> some View {
        if !i.genres.isEmpty {
            fact("Genres") {
                FlowTags(items: i.genres)
            }
        }
    }

    private func fact<V: View>(_ label: String, @ViewBuilder _ v: () -> V) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 12) {
            Text(label).font(Inter.regular(13)).foregroundStyle(p.faint).frame(width: 96, alignment: .leading)
            v().frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

/// Genre chips that wrap.
struct FlowTags: View {
    let items: [String]

    var body: some View {
        FlowLayout(spacing: 6) {
            ForEach(items, id: \.self) { Tag(text: $0) }
        }
    }
}

struct FlowLayout: Layout {
    var spacing: CGFloat = 6

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let rows = arrange(proposal.width ?? .infinity, subviews)
        return CGSize(width: proposal.width ?? rows.width, height: rows.height)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var x = bounds.minX, y = bounds.minY, line: CGFloat = 0
        for v in subviews {
            let s = v.sizeThatFits(.unspecified)
            if x > bounds.minX && x + s.width > bounds.maxX {
                x = bounds.minX
                y += line + spacing
                line = 0
            }
            v.place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(s))
            x += s.width + spacing
            line = max(line, s.height)
        }
    }

    private func arrange(_ width: CGFloat, _ subviews: Subviews) -> (width: CGFloat, height: CGFloat) {
        var x: CGFloat = 0, y: CGFloat = 0, line: CGFloat = 0, widest: CGFloat = 0
        for v in subviews {
            let s = v.sizeThatFits(.unspecified)
            if x > 0 && x + s.width > width {
                y += line + spacing
                x = 0
                line = 0
            }
            x += s.width + spacing
            widest = max(widest, x - spacing)
            line = max(line, s.height)
        }
        return (widest, y + line)
    }
}

/// "Your list" for a title that is not on it: "Add as Watching", then "Add as" the other statuses,
/// as the desktop's details page.
struct AddToListCard: View {
    @Environment(\.palette) private var p
    let kind: MediaKind
    let service: String
    let add: (ListStatus) -> Void

    var body: some View {
        Card {
            HStack {
                Text("Your list").font(Inter.semibold(15)).foregroundStyle(p.text)
                Spacer()
                Text(service).font(Inter.regular(12)).foregroundStyle(p.faint)
            }
            .padding(.horizontal, 16).padding(.top, 14).padding(.bottom, 4)
            VStack(alignment: .leading, spacing: 10) {
                Text("Not on your list.").font(Inter.regular(14)).foregroundStyle(p.dim)
                Button { add(.watching) } label: {
                    Text("Add as \(ListStatus.watching.label(kind))").font(Inter.semibold(15)).foregroundStyle(p.onAccent)
                        .frame(maxWidth: .infinity).frame(height: 44)
                        .background(RoundedRectangle(cornerRadius: 10).fill(p.accent))
                }
                .buttonStyle(.borderless)
                .accessibilityIdentifier("details/add-watching")
                FlowLayout(spacing: 6) {
                    ForEach([ListStatus.planToWatch, .completed, .onHold, .dropped], id: \.self) { st in
                        Button { add(st) } label: {
                            HStack(spacing: 6) {
                                Circle().fill(p.status(st)).frame(width: 8, height: 8)
                                Text(st.label(kind)).font(Inter.regular(13)).foregroundStyle(p.dim)
                            }
                            .padding(.horizontal, 12).frame(height: 32)
                            .overlay(Capsule().stroke(p.borderStrong, lineWidth: 1))
                            .contentShape(Capsule())
                        }
                        .buttonStyle(.borderless)
                        .accessibilityLabel("Add as \(st.label(kind))")
                    }
                }
                Text("It goes to the sync queue first, then to \(service).").font(Inter.regular(12)).foregroundStyle(p.faint)
            }
            .padding(.horizontal, 16).padding(.top, 4).padding(.bottom, 14)
        }
    }
}

extension AppModel {
    /// "Add as …" for a title not on the list (details page, Airing): saved at once, sent through
    /// the queue.
    func addTitle(_ stub: TitleStub, _ status: ListStatus) {
        guard let repo = repo as? DetailsRepository else { return }
        Task {
            do {
                try await repo.addTitle(stub, status: status)
                await reload()
                requestFlush()
            } catch {
                snack = Snack(text: message(of: error))
            }
        }
    }
}

extension Notification.Name {
    /// A notification was tapped: open its entry (`object` is a `RowRef`).
    static let kotoraOpenEntry = Notification.Name("kotoraOpenEntry")
}
