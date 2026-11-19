import SwiftUI

/// The search page's state, as the desktop's (`app/src/search.rs`): the user's own entries that
/// match (every status) at once, then the service's results that are not on the list, asked
/// after a pause in typing, for 3 letters or more, kept 10 minutes (the last 20 queries).
@MainActor
final class SearchModel: ObservableObject {
    static let debounce: Duration = .milliseconds(400)
    static let cacheSecs: Int64 = 600
    static let cacheMax = 20

    @Published var text = ""
    @Published var kind: MediaKind = .anime
    /// The service's results for `asked` (all of them; those on the list are left out when shown).
    @Published private(set) var hits: [Hit] = []
    @Published private(set) var asked = ""
    @Published private(set) var pending = false
    @Published private(set) var error: String?
    /// The whole list of `kind`, for "On your list" and to leave out what is on it.
    @Published private(set) var mine: [Row] = []

    private var cache: [(kind: MediaKind, query: String, at: Int64, hits: [Hit])] = []
    private var task: Task<Void, Never>?

    static func norm(_ q: String) -> String {
        q.split(whereSeparator: \.isWhitespace).joined(separator: " ").lowercased()
    }

    var query: String { Self.norm(text) }

    /// "On your list": every title word in the title or the English title, as the desktop's filter.
    var onList: [Row] {
        let words = query.split(separator: " ")
        guard !words.isEmpty else { return [] }
        return mine.filter { r in
            let t = (r.title + "\n" + (r.titleEn ?? "")).lowercased()
            return words.allSatisfy { t.contains($0) }
        }
    }

    /// Results that are not on the list (those are shown above them).
    var newHits: [Hit] {
        hits.filter { h in !mine.contains { $0.id == h.id } }
    }

    func loadMine(_ model: AppModel) async {
        guard let r = model.repo else { return }
        mine = (try? await r.list(kind: kind, status: nil, sort: .title)) ?? []
    }

    /// Every edit of the box (and the Anime | Manga switch): the list part at once, the service
    /// after a pause.
    func changed(_ model: AppModel) {
        let q = query, kind = kind
        if q == asked && (pending || error != nil || !hits.isEmpty) { return }
        task?.cancel()
        asked = q
        error = nil
        guard q.count >= 3, model.expired == nil, !model.list.offline, let repo = model.repo as? SearchRepository else {
            hits = []
            pending = false
            return
        }
        let now = nowSeconds()
        cache.removeAll { now - $0.at >= Self.cacheSecs }
        if let c = cache.first(where: { $0.kind == kind && $0.query == q }) {
            hits = c.hits
            pending = false
            return
        }
        hits = []
        pending = true
        task = Task {
            try? await Task.sleep(for: Self.debounce)
            if Task.isCancelled { return }
            do {
                let found = try await repo.search(kind: kind, query: q)
                if Task.isCancelled { return }
                cache.removeAll { $0.kind == kind && $0.query == q }
                cache.append((kind, q, nowSeconds(), found))
                if cache.count > Self.cacheMax { cache.removeFirst() }
                hits = found
            } catch {
                if Task.isCancelled { return }
                self.error = message(of: error)
            }
            pending = false
        }
    }

    /// "Try again" after an error: forget it and ask again.
    func again(_ model: AppModel) {
        error = nil
        asked = ""
        changed(model)
    }

    /// "+ Watching" or another status on a result. Saved at once, sent through the queue.
    func add(_ hit: Hit, _ status: ListStatus, _ model: AppModel) {
        guard let repo = model.repo as? SearchRepository else { return }
        Task {
            do {
                try await repo.add(hit, status: status)
                await loadMine(model)
                await model.reload()
                if let r = model.repo, !r.isSample { model.flusher.request(r) }
            } catch {
                model.snack = Snack(text: message(of: error))
            }
        }
    }
}

/// Search: Anime | Manga (Manga on), the box, "On your list", then "On MyAnimeList" (AniList)
/// with "+ Watching" and the other statuses in a menu, as the desktop's search page.
struct SearchScreen: View {
    @Environment(\.palette) private var p
    @EnvironmentObject private var model: AppModel
    @StateObject private var s = SearchModel()
    @State private var path: [RowRef] = []
    @FocusState private var focused: Bool

    private var service: String { model.session.active?.service.label ?? "MyAnimeList" }

    var body: some View {
        NavigationStack(path: $path) {
            VStack(spacing: 0) {
                AppBar(title: "Search") { AccountMenu() }
                if model.mangaEnabled {
                    Segmented(items: [(MediaKind.anime, "Anime"), (.manga, "Manga")], selection: $s.kind)
                        .padding(.horizontal, 16).padding(.bottom, 10)
                }
                field
                results
            }
            .background(p.bg)
            .toolbar(.hidden, for: .navigationBar)
            .navigationDestination(for: RowRef.self) { DetailsScreen(ref: $0) }
        }
        .task {
            await s.loadMine(model)
            #if DEBUG
            // `-query <words>`: a search at launch, for screenshots.
            if let q = UserDefaults.standard.string(forKey: "query"), s.text.isEmpty { s.text = q }
            #endif
        }
        .onChange(of: s.text) { s.changed(model) }
        .onChange(of: s.kind) {
            Task {
                await s.loadMine(model)
                s.changed(model)
            }
        }
        .onChange(of: model.list.version) { Task { await s.loadMine(model) } }
        .onChange(of: model.mangaEnabled) { _, on in if !on { s.kind = .anime } }
    }

    private var field: some View {
        HStack(spacing: 8) {
            Image(systemName: "magnifyingglass").font(.system(size: 15)).foregroundStyle(p.faint)
            TextField("", text: $s.text, prompt: Text(s.kind == .manga ? "Search manga" : "Search anime").foregroundStyle(p.faint))
                .font(Inter.regular(15)).foregroundStyle(p.text)
                .focused($focused).submitLabel(.search)
                .autocorrectionDisabled().textInputAutocapitalization(.never)
            if !s.text.isEmpty {
                Button { s.text = "" } label: { Image(systemName: "xmark.circle.fill").foregroundStyle(p.faint) }
                    .accessibilityLabel("Clear")
            }
        }
        .padding(.horizontal, 12).frame(height: 42)
        .background(RoundedRectangle(cornerRadius: 10).fill(p.field))
        .overlay(RoundedRectangle(cornerRadius: 10).stroke(focused ? p.accent : p.borderStrong, lineWidth: 1))
        .padding(.horizontal, 16).padding(.bottom, 6)
    }

    @ViewBuilder private var results: some View {
        let q = s.text.trimmingCharacters(in: .whitespaces)
        List {
            if q.isEmpty {
                Text(s.kind == .manga ? "Find a manga on your list or on \(service), and add it." : "Find an anime on your list or on \(service), and add it.")
                    .font(Inter.regular(14)).foregroundStyle(p.faint).multilineTextAlignment(.center)
                    .frame(maxWidth: .infinity).padding(.top, 40).padding(.horizontal, 16)
                    .plain(p)
            } else {
                if let pid = (model.repo as? DetailsRepository)?.pastedId(kind: s.kind, text: q) { idLine(pid.id) }
                let mine = s.onList
                head("On your list · \(mine.count)", nil)
                if mine.isEmpty {
                    note("Nothing on your list matches “\(q)”.")
                }
                ForEach(mine, id: \.key) { row in
                    RowView(row: row) { model.plusOne(row) }
                        .contentShape(Rectangle())
                        .onTapGesture { path.append(RowRef(kind: row.kind, id: row.id)) }
                        .listRowInsets(EdgeInsets(top: 0, leading: 16, bottom: 0, trailing: 12))
                        .listRowBackground(p.bg).listRowSeparatorTint(p.border)
                }
                online(q)
            }
            Color.clear.frame(height: 60).plain(p)
        }
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
        .scrollDismissesKeyboard(.immediately)
    }

    @ViewBuilder private func online(_ q: String) -> some View {
        let fresh = s.newHits
        let end: String? = s.pending ? "Searching…" : (s.error == nil && s.asked.count >= 3 && !s.hits.isEmpty ? "\(s.hits.count) found" : nil)
        head("On \(service)", end)
        if let blocked = blocked {
            note(blocked)
        } else if s.pending {
            note("Searching \(service)…")
        } else if let e = s.error {
            HStack(spacing: 10) {
                Text(e).font(Inter.regular(14)).foregroundStyle(p.dropped)
                Spacer()
                Button("Try again") { s.again(model) }.font(Inter.medium(14)).foregroundStyle(p.accent).buttonStyle(.borderless)
            }
            .padding(.vertical, 8).plain(p)
        } else if fresh.isEmpty {
            note(s.hits.isEmpty ? "\(service) found nothing for “\(q)”." : "Everything \(service) found is on your list already.")
        } else {
            ForEach(fresh, id: \.id) { h in
                HitView(hit: h) { s.add(h, $0, model) }
                    .contentShape(Rectangle())
                    .onTapGesture { path.append(RowRef(kind: h.kind, id: h.id, stub: h.stub)) }
                    .listRowInsets(EdgeInsets(top: 0, leading: 16, bottom: 0, trailing: 12))
                    .listRowBackground(p.bg).listRowSeparatorTint(p.border)
            }
        }
    }

    /// "Open MyAnimeList #41590: Title": a pasted link or a bare number may be an id (the desktop's
    /// id line).
    private func idLine(_ id: UInt32) -> some View {
        let known = s.mine.first { $0.id == id }?.title ?? s.hits.first { $0.id == id }?.title
        let what = known.map { "\(service) #\(id): \($0)" } ?? "\(service) #\(id)"
        return Button {
            let hit = s.hits.first { $0.id == id }
            path.append(RowRef(kind: s.kind, id: id, stub: hit?.stub))
        } label: {
            HStack(spacing: 10) {
                Image(systemName: "arrow.right").font(.system(size: 14, weight: .semibold)).foregroundStyle(p.accentHi)
                Text("Open \(what)").font(Inter.medium(15)).foregroundStyle(p.text).lineLimit(1)
                Spacer()
            }
            .frame(height: 44).contentShape(Rectangle())
        }
        .buttonStyle(.borderless)
        .accessibilityLabel("Open \(service) \(s.kind == .manga ? "manga" : "anime") \(id)")
        .accessibilityIdentifier("search/id-line")
        .plain(p)
    }

    /// Why the service isn't searched now, if it isn't (the desktop's words).
    private var blocked: String? {
        if model.expired != nil { return "Sign in again to search \(service)." }
        if model.list.offline { return "Offline: searching \(service) needs the network. Your list is searched above." }
        if s.query.count < 3 { return "Type 3 letters or more to search \(service) too." }
        return nil
    }

    private func head(_ title: String, _ end: String?) -> some View {
        HStack {
            Text(title).font(Inter.semibold(13)).foregroundStyle(p.dim)
            Spacer()
            if let end { Text(end).font(Inter.regular(12)).foregroundStyle(p.faint) }
        }
        .padding(.top, 14).padding(.bottom, 2)
        .plain(p)
    }

    private func note(_ text: String) -> some View {
        Text(text).font(Inter.regular(14)).foregroundStyle(p.faint).padding(.vertical, 8).plain(p)
    }
}

private extension View {
    func plain(_ p: Palette) -> some View {
        listRowBackground(p.bg).listRowSeparator(.hidden)
            .listRowInsets(EdgeInsets(top: 0, leading: 16, bottom: 0, trailing: 16))
    }
}

/// A service's result: cover, title, type and what it is, the second title, and the add buttons.
struct HitView: View {
    @Environment(\.palette) private var p
    let hit: Hit
    let add: (ListStatus) -> Void

    var body: some View {
        HStack(spacing: 12) {
            CoverTile(id: hit.id, initials: initials, url: hit.coverUrl, width: 44, height: 64, radius: 5)
            VStack(alignment: .leading, spacing: 5) {
                Text(hit.title).font(Inter.medium(15)).foregroundStyle(p.text).lineLimit(2)
                HStack(spacing: 6) {
                    if !hit.typeLabel.isEmpty { Tag(text: hit.typeLabel) }
                    Text(hit.meta).font(Inter.regular(12.5)).monospacedDigit().foregroundStyle(p.faint).lineLimit(2)
                }
                if let en = hit.titleEn, !en.isEmpty, en.lowercased() != hit.title.lowercased() {
                    Text(en).font(Inter.regular(12.5)).foregroundStyle(p.dim).lineLimit(1)
                }
            }
            Spacer(minLength: 4)
            HStack(spacing: 0) {
                Button { add(.watching) } label: {
                    HStack(spacing: 4) {
                        Image(systemName: "plus").font(.system(size: 12, weight: .semibold))
                        Text(ListStatus.watching.label(hit.kind)).font(Inter.semibold(13))
                    }
                    .foregroundStyle(p.accent).padding(.horizontal, 10).frame(height: 34)
                }
                .buttonStyle(.borderless)
                .accessibilityLabel("Add as \(ListStatus.watching.label(hit.kind))")
                Rectangle().fill(p.accent.opacity(0.4)).frame(width: 1, height: 20)
                Menu {
                    Section("Add as") {
                        ForEach([ListStatus.planToWatch, .completed, .onHold, .dropped], id: \.self) { st in
                            Button(st.label(hit.kind)) { add(st) }
                        }
                    }
                } label: {
                    Image(systemName: "chevron.down").font(.system(size: 12, weight: .semibold))
                        .foregroundStyle(p.accent).frame(width: 30, height: 34)
                }
                .accessibilityLabel("Add with another status")
            }
            .overlay(RoundedRectangle(cornerRadius: 8).stroke(p.accent.opacity(0.6), lineWidth: 1))
        }
        .frame(minHeight: 84)
        .accessibilityElement(children: .contain)
    }

    private var initials: String {
        let words = hit.title.split(whereSeparator: { $0 == " " || $0 == "-" || $0 == ":" })
            .filter { $0.first.map { $0.isLetter || $0.isNumber } == true }
        return String(words.prefix(2).compactMap(\.first)).uppercased()
    }
}

extension Hit {
    /// What the details page shows before the service answers.
    var stub: TitleStub {
        TitleStub(kind: kind, id: id, title: title, coverUrl: coverUrl, format: format, episodes: episodes)
    }
}
