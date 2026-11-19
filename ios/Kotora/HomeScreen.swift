import SwiftUI

/// Home, as the desktop's (`app/src/home.rs`): the list at a glance, in shelves. Continue watching
/// (wide cards with the next episode's +1), Continue reading with Manga on (+1 is the next
/// chapter), Airing this week (the next episode of the list's airing shows, today's chip filled)
/// and Plan to watch. Each shelf scrolls sideways and links to its full page ("See all"). Only
/// reads what is saved.
struct HomeScreen: View {
    @Environment(\.palette) private var p
    @EnvironmentObject private var model: AppModel
    @Binding var path: [RowRef]
    /// "See all": the list page at that kind and status.
    let seeAll: (MediaKind, ListStatus) -> Void
    /// Airing this week's "See all": the Airing page.
    var seeAiring: () -> Void = {}
    @State private var shelves = Shelves()

    /// Cards a shelf holds at most; "See all" opens the rest.
    static let shelfMax = 20

    struct Shelves: Equatable {
        var watching: [Row] = []
        var reading: [Row] = []
        var plan: [Row] = []
        var airing: [HomeAiring] = []
        var loaded = false
    }

    var body: some View {
        VStack(spacing: 0) {
            AppBar(title: "Home") { AccountMenu() }
            ScrollView {
                VStack(alignment: .leading, spacing: 26) {
                    shelf("Continue watching", shelves.watching, all: (.anime, .watching),
                          none: "Nothing to continue: start something from Plan to watch.") { WideCard(row: $0) { model.plusOne($0) } }
                    if model.mangaEnabled {
                        shelf("Continue reading", shelves.reading, all: (.manga, .watching),
                              none: "Nothing to continue: start something from Plan to read.") { WideCard(row: $0) { model.plusOne($0) } }
                    }
                    airingShelf
                    shelf("Plan to watch", shelves.plan, all: (.anime, .planToWatch),
                          none: "Your Plan to watch list is empty.") { PosterCard(row: $0) }
                    HomeStatsCard()
                }
                .padding(.top, 8).padding(.bottom, 80)
            }
            .refreshable { await model.refresh() }
        }
        .background(p.bg)
        .task(id: model.list.version) { await load() }
        .onChange(of: model.mangaEnabled) { Task { await load() } }
    }

    private func load() async {
        guard let r = model.repo else { return }
        var s = Shelves()
        let anime = (try? await r.list(kind: .anime, status: nil, sort: .updated)) ?? []
        // Watching (and rewatches) with a next episode, last updated first.
        s.watching = anime.filter(Self.continuing)
        s.plan = anime.filter { $0.status == .planToWatch }
        if model.mangaEnabled {
            let manga = (try? await r.list(kind: .manga, status: nil, sort: .updated)) ?? []
            s.reading = manga.filter(Self.continuing)
        }
        if let src = r as? ExtrasSource { s.airing = (try? await src.homeAiring()) ?? [] }
        s.loaded = true
        shelves = s
    }

    /// Airing this week: the poster with the episode chip, the title and when it airs.
    private var airingShelf: some View {
        let title = "Airing this week"
        let items = shelves.airing
        return VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .lastTextBaseline, spacing: 8) {
                Text(title).font(Inter.semibold(17)).foregroundStyle(p.text)
                Text("\(items.count)").font(Inter.regular(13)).monospacedDigit().foregroundStyle(p.faint)
                Spacer()
                Button { seeAiring() } label: {
                    HStack(spacing: 4) {
                        Text("See all").font(Inter.medium(13))
                        Image(systemName: "arrow.right").font(.system(size: 11, weight: .semibold))
                    }
                    .foregroundStyle(p.dim).padding(.vertical, 4)
                }
                .accessibilityLabel("See all: \(title)")
            }
            .padding(.horizontal, 16)
            if items.isEmpty {
                Text(shelves.loaded ? "None of your shows airs in the next seven days." : " ")
                    .font(Inter.regular(13)).foregroundStyle(p.faint).padding(.horizontal, 16)
            } else {
                ScrollView(.horizontal, showsIndicators: false) {
                    LazyHStack(alignment: .top, spacing: 12) {
                        ForEach(items.prefix(Self.shelfMax), id: \.id) { a in
                            AiringPoster(item: a)
                                .contentShape(Rectangle())
                                .onTapGesture { path.append(RowRef(kind: .anime, id: a.id)) }
                                .accessibilityAddTraits(.isButton)
                        }
                    }
                    .padding(.horizontal, 16)
                }
            }
        }
    }

    /// Continue watching / reading: Watching (and rewatches) with a next episode or chapter.
    static func continuing(_ r: Row) -> Bool {
        (r.status == .watching || r.rewatching) && (r.total.map { r.progress < $0 } ?? true)
    }

    /// A shelf: heading with its count and "See all", then the cards in a row that scrolls sideways.
    private func shelf<Card: View>(_ title: String, _ rows: [Row], all: (MediaKind, ListStatus), none: String,
                                   @ViewBuilder card: @escaping (Row) -> Card) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .lastTextBaseline, spacing: 8) {
                Text(title).font(Inter.semibold(17)).foregroundStyle(p.text)
                Text("\(rows.count)").font(Inter.regular(13)).monospacedDigit().foregroundStyle(p.faint)
                Spacer()
                Button { seeAll(all.0, all.1) } label: {
                    HStack(spacing: 4) {
                        Text("See all").font(Inter.medium(13))
                        Image(systemName: "arrow.right").font(.system(size: 11, weight: .semibold))
                    }
                    .foregroundStyle(p.dim).padding(.vertical, 4)
                }
                .accessibilityLabel("See all: \(title)")
            }
            .padding(.horizontal, 16)
            if rows.isEmpty {
                Text(shelves.loaded ? none : " ").font(Inter.regular(13)).foregroundStyle(p.faint).padding(.horizontal, 16)
            } else {
                ScrollView(.horizontal, showsIndicators: false) {
                    LazyHStack(alignment: .top, spacing: 12) {
                        ForEach(rows.prefix(Self.shelfMax), id: \.key) { row in
                            card(row)
                                .contentShape(Rectangle())
                                .onTapGesture { path.append(RowRef(kind: row.kind, id: row.id)) }
                                .accessibilityAddTraits(.isButton)
                        }
                    }
                    .padding(.horizontal, 16)
                }
            }
        }
    }
}

/// Continue watching / reading: the cover behind, the poster, the title, how far, the bar and +1.
struct WideCard: View {
    @Environment(\.palette) private var p
    let row: Row
    let plusOne: (Row) -> Void

    var body: some View {
        let manga = row.kind == .manga
        let next = row.progress + 1
        HStack(alignment: .top, spacing: 12) {
            CoverTile(id: row.id, initials: row.initials, url: row.coverUrl, width: 70, height: 104, radius: 6)
            VStack(alignment: .leading, spacing: 4) {
                Text(row.title).font(Inter.semibold(14)).foregroundStyle(p.text).lineLimit(2)
                Text(progressText).font(Inter.medium(12.5)).monospacedDigit().foregroundStyle(p.dim).lineLimit(1)
                Spacer(minLength: 4)
                HStack(spacing: 8) {
                    GeometryReader { g in
                        ZStack(alignment: .leading) {
                            Capsule().fill(p.text.opacity(0.16))
                            Capsule().fill(p.status(row.rewatching ? .completed : row.status)).frame(width: g.size.width * fraction)
                        }
                    }
                    .frame(height: 4)
                    Button { plusOne(row) } label: {
                        Text("+1  \(manga ? "Ch" : "Ep") \(next)").font(Inter.semibold(13)).foregroundStyle(p.text)
                            .padding(.horizontal, 10).frame(height: 30)
                            .background(RoundedRectangle(cornerRadius: 7).fill(p.bg.opacity(0.7)))
                            .overlay(RoundedRectangle(cornerRadius: 7).stroke(p.borderStrong, lineWidth: 1))
                    }
                    .buttonStyle(.borderless)
                    .accessibilityLabel("+1, \(manga ? "chapter" : "episode") \(next)")
                }
            }
        }
        .padding(10)
        .frame(width: 300, height: 124)
        .background {
            ZStack {
                p.surface
                CoverTile(id: row.id, initials: "", url: row.coverUrl, width: nil, height: 124, radius: 0)
                    .blur(radius: 18).opacity(0.35)
            }
        }
        .clipShape(RoundedRectangle(cornerRadius: 14))
        .overlay(RoundedRectangle(cornerRadius: 14).stroke(p.border, lineWidth: 1))
        .accessibilityLabel("About \(row.title)")
    }

    /// "Ep 6 / 12", "Ch 21 / ?".
    private var progressText: String {
        let unit = row.kind == .manga ? "Ch" : "Ep"
        return "\(unit) \(row.progress) / \(row.total.map(String.init) ?? "?")"
    }

    private var fraction: CGFloat {
        guard let t = row.total, t > 0 else { return row.progress > 0 ? 0.15 : 0 }
        return min(1, CGFloat(row.progress) / CGFloat(t))
    }
}

/// Plan to watch: the poster, the title, what it is.
struct PosterCard: View {
    @Environment(\.palette) private var p
    let row: Row

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            CoverTile(id: row.id, initials: row.initials, url: row.coverUrl, width: 110, height: 165, radius: 8)
            Text(row.title).font(Inter.medium(13)).foregroundStyle(p.text).lineLimit(2, reservesSpace: true)
            Text(line).font(Inter.regular(12)).foregroundStyle(p.faint).lineLimit(1)
        }
        .frame(width: 110)
        .accessibilityElement(children: .combine)
    }

    /// "TV · 12 ep", as the desktop's card.
    private var line: String {
        let f = row.formatLabel
        guard let n = row.total else { return f }
        return f.isEmpty ? "\(n) ep" : "\(f) · \(n) ep"
    }
}

/// An Airing this week card: the poster with "Ep N" (filled today), the title, "Today 16:30".
struct AiringPoster: View {
    @Environment(\.palette) private var p
    let item: HomeAiring

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            CoverTile(id: item.id, initials: item.title.initialsOfTitle, url: item.coverUrl, width: 110, height: 165, radius: 8)
                .overlay(alignment: .topLeading) {
                    Text(item.episode).font(Inter.semibold(11)).foregroundStyle(item.today ? p.onAccent : p.text)
                        .padding(.horizontal, 6).padding(.vertical, 2)
                        .background(RoundedRectangle(cornerRadius: 6).fill(item.today ? p.accent : p.bg.opacity(0.75)))
                        .padding(6)
                }
            Text(item.title).font(Inter.medium(13)).foregroundStyle(p.text).lineLimit(2, reservesSpace: true)
            Text(item.when).font(Inter.regular(12)).foregroundStyle(item.today ? p.accent : p.faint).lineLimit(1)
        }
        .frame(width: 110)
        .accessibilityElement(children: .combine)
    }
}
