import SwiftUI

/// Publishing (Manga mode, in Airing's place), as on the desktop: for each Reading manga, the
/// latest chapter out and when, the chapters left to read, and an estimate of the next release,
/// from MangaUpdates and MangaDex (`kotora-mobile`, the desktop's words). Opening the page asks
/// only for what is older than four hours; pull down asks again for all. A row opens its panel:
/// the matched series, recent releases, the series' page, and a pasted link to fix the match.
struct PublishingScreen: View {
    @Environment(\.palette) private var p
    @EnvironmentObject private var model: AppModel
    @Binding var path: [RowRef]

    @State private var sort: PublishingSort = .next
    @State private var page: PublishingPage?
    @State private var busy = false
    @State private var error: String?
    @State private var open: PublishingRow?

    var body: some View {
        VStack(spacing: 0) {
            AppBar(title: "Publishing") {
                if busy { ProgressView().tint(p.dim).frame(width: 40, height: 40) }
                else {
                    Button { Task { await refresh(force: true) } } label: {
                        Image(systemName: "arrow.triangle.2.circlepath").font(.system(size: 18)).foregroundStyle(p.dim).frame(width: 40, height: 40)
                    }
                    .accessibilityLabel("Check MangaUpdates again")
                }
                AccountMenu()
            }
            status
            if let error {
                Banner(text: (page?.checkedAt != nil) ? "\(error) Showing the saved releases." : error, retry: { Task { await refresh(force: true) } })
            }
            if let page, !page.rows.isEmpty {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 6) {
                        ForEach([(PublishingSort.next, "Next release"), (.behind, "Most to read"), (.latest, "Latest release")], id: \.0) { s in
                            Chip(text: s.1, on: sort == s.0) { sort = s.0 }
                        }
                    }
                    .padding(.horizontal, 16).padding(.vertical, 10)
                }
                List {
                    ForEach(page.rows, id: \.id) { row($0) }
                    Text("\(page.credit) · Next dates are estimates from past releases, not announcements.")
                        .font(Inter.regular(12)).foregroundStyle(p.faint).multilineTextAlignment(.center)
                        .frame(maxWidth: .infinity).padding(.vertical, 12)
                        .listRowBackground(p.bg).listRowSeparator(.hidden)
                    Color.clear.frame(height: 40).listRowBackground(p.bg).listRowSeparator(.hidden)
                }
                .listStyle(.plain)
                .scrollContentBackground(.hidden)
                .refreshable { await refresh(force: true) }
            } else if page != nil {
                VStack(spacing: 4) {
                    Spacer()
                    Text("No manga in Reading").font(Inter.semibold(17)).foregroundStyle(p.text)
                    Text("Manga you’re reading show their latest chapters here.").font(Inter.regular(13.5)).foregroundStyle(p.dim)
                    Spacer()
                }
                .frame(maxWidth: .infinity).padding(.horizontal, 32)
            } else {
                Spacer()
            }
        }
        .background(p.bg)
        .task { await start() }
        .task(id: model.list.version) { await load() }
        .onChange(of: sort) { Task { await load() } }
        .sheet(item: $open) { r in
            PublishingPanel(row: r) { Task { await load() } }
                .presentationDetents([.medium, .large])
        }
    }

    @ViewBuilder private var status: some View {
        if busy {
            Text("Checking MangaUpdates…").font(Inter.regular(12.5)).foregroundStyle(p.faint).padding(.bottom, 4)
        } else if let at = page?.checkedAt {
            Text("Checked \(ago(at))").font(Inter.regular(12.5)).foregroundStyle(p.faint).padding(.bottom, 4)
        }
    }

    private func row(_ r: PublishingRow) -> some View {
        HStack(spacing: 12) {
            CoverTile(id: r.id, initials: String(r.title.split(separator: " ").prefix(2).compactMap(\.first)).uppercased(), url: r.coverUrl, width: 44, height: 64, radius: 5)
                .onTapGesture { path.append(RowRef(kind: .manga, id: r.id)) }
            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: 6) {
                    Text(r.title).font(Inter.medium(15)).foregroundStyle(p.text).lineLimit(1)
                    if r.look == .guess { Tag(text: "Check match", style: .wait) }
                    if r.error != nil { Tag(text: "Couldn\u{2019}t check", style: .wait) }
                }
                latest(r)
                next(r)
            }
            Spacer(minLength: 4)
            VStack(alignment: .trailing, spacing: 6) {
                behind(r)
                Button { plusOne(r) } label: {
                    Text("+1").font(Inter.semibold(13)).foregroundStyle(p.text)
                        .frame(width: 44, height: 30)
                        .background(RoundedRectangle(cornerRadius: 8).fill(p.raised))
                        .overlay(RoundedRectangle(cornerRadius: 8).stroke(p.borderStrong, lineWidth: 1))
                }
                .buttonStyle(.borderless)
                .accessibilityLabel("Mark chapter \(r.progress + 1) of \(r.title) read")
            }
        }
        .padding(.vertical, 8)
        .contentShape(Rectangle())
        .onTapGesture { open = r }
        .listRowInsets(EdgeInsets(top: 0, leading: 16, bottom: 0, trailing: 12))
        .listRowBackground(p.bg)
        .listRowSeparatorTint(p.border)
    }

    @ViewBuilder private func latest(_ r: PublishingRow) -> some View {
        if r.latest.isEmpty {
            Text(r.look == .pending && !busy ? "–" : (r.look == .pending ? "Looking…" : (r.look == .notFound ? "Not found on the release sources" : "–")))
                .font(Inter.regular(12.5)).foregroundStyle(p.faint)
        } else {
            HStack(spacing: 6) {
                Text(r.latest).font(Inter.semibold(13)).monospacedDigit().foregroundStyle(r.fresh ? p.accentHi : p.text)
                Text(r.when).font(Inter.regular(12.5)).foregroundStyle(r.ahead ? p.onHold : p.dim)
            }
            .lineLimit(1)
        }
    }

    @ViewBuilder private func next(_ r: PublishingRow) -> some View {
        if !r.pace.isEmpty {
            HStack(spacing: 6) {
                Text(r.pace).font(Inter.regular(12.5)).foregroundStyle(r.hiatus ? p.dropped : (r.finished ? p.dim : p.text))
                if !r.next.isEmpty {
                    Text(r.next).font(Inter.regular(12.5))
                        .foregroundStyle(r.next == "late" ? p.onHold : (r.next == "due today" ? p.accentHi : p.dim))
                }
            }
            .lineLimit(1)
        }
    }

    @ViewBuilder private func behind(_ r: PublishingRow) -> some View {
        switch r.behind {
        case .some(0): Text("Caught up").font(Inter.regular(12.5)).foregroundStyle(p.dim)
        case .some(let n):
            HStack(alignment: .lastTextBaseline, spacing: 3) {
                Text("\(n)").font(Inter.semibold(15)).monospacedDigit().foregroundStyle(p.text)
                Text("to read").font(Inter.regular(12)).foregroundStyle(p.dim)
            }
        case .none: Text("–").font(Inter.regular(12.5)).foregroundStyle(p.faint)
        }
    }

    private func ago(_ at: Int64) -> String {
        let s = max(0, Int64(Date().timeIntervalSince1970) - at)
        switch s {
        case ..<60: return "just now"
        case ..<3600: return "\(s / 60) min ago"
        case ..<86_400: return "\(s / 3600) h ago"
        default: return "\(s / 86_400) d ago"
        }
    }

    private func plusOne(_ r: PublishingRow) {
        Task {
            if let row = await model.entry(.manga, r.id) { model.plusOne(row) }
        }
    }

    // ----------------------------------------------------------------------------------- data

    private func start() async {
        await load()
        Task { await Notifier.askPermission() }
        if page?.stale ?? false { await refresh(force: false) }
    }

    private func load() async {
        guard let src = model.repo as? PagesSource else { page = PublishingPage(rows: [], checkedAt: nil, credit: "", stale: false); return }
        do { page = try await src.publishingPage(sort: sort) } catch { self.error = message(of: error) }
    }

    private func refresh(force: Bool) async {
        guard let src = model.repo as? PagesSource, !busy else { return }
        busy = true
        defer { busy = false }
        do {
            let r = try await src.publishingRefresh(force: force)
            error = r.error
            if !r.notices.isEmpty { await Notifier.check(chapters: false) }
        } catch {
            self.error = message(of: error)
        }
        await load()
    }
}

extension PublishingRow: Identifiable {}

/// A row's panel: the matched series, recent releases, the series' page, and a pasted link.
struct PublishingPanel: View {
    @Environment(\.palette) private var p
    @Environment(\.openURL) private var openURL
    @EnvironmentObject private var model: AppModel
    let row: PublishingRow
    let changed: () -> Void

    @State private var link = ""
    @State private var note: (failed: Bool, text: String)?
    @State private var working = false

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                Text(row.title).font(Inter.semibold(18)).foregroundStyle(p.text)
                if let t = row.seriesTitle {
                    HStack(spacing: 6) {
                        Text("\(t) on \(row.source ?? "MangaUpdates")").font(Inter.regular(13.5)).foregroundStyle(p.dim)
                        if row.look == .guess { Tag(text: "Check match", style: .wait) }
                        if row.look == .picked { Tag(text: "Your link") }
                    }
                    if let u = row.seriesUrl, let url = URL(string: u) {
                        Button("Open the series page") { openURL(url) }.font(Inter.medium(13.5)).foregroundStyle(p.accentHi)
                    }
                } else {
                    Text(row.look == .notFound ? "No series found on MangaUpdates or MangaDex." : "Not looked up yet.")
                        .font(Inter.regular(13.5)).foregroundStyle(p.dim)
                }
                if let e = row.error { Text(e).font(Inter.regular(12.5)).foregroundStyle(p.dropped) }
                if !row.recent.isEmpty {
                    SectionTitle(text: "Recent releases").padding(.horizontal, -20)
                    ForEach(Array(row.recent.enumerated()), id: \.offset) { _, r in
                        HStack { Text(r.label).font(Inter.medium(13.5)).foregroundStyle(p.text); Spacer(); Text(r.when).font(Inter.regular(12.5)).foregroundStyle(p.dim) }
                    }
                }
                SectionTitle(text: "Wrong series?").padding(.horizontal, -20)
                Text("Paste its mangaupdates.com/series/… or mangadex.org/title/… address.").font(Inter.regular(12.5)).foregroundStyle(p.faint)
                TextField("Series link", text: $link)
                    .textInputAutocapitalization(.never).autocorrectionDisabled().keyboardType(.URL)
                    .font(Inter.regular(14)).padding(10)
                    .background(RoundedRectangle(cornerRadius: 8).fill(p.field))
                    .overlay(RoundedRectangle(cornerRadius: 8).stroke(p.borderStrong, lineWidth: 1))
                HStack(spacing: 10) {
                    Button("Use") { act { try await $0.publishingLink(id: row.id, link: link) } }
                        .disabled(link.trimmingCharacters(in: .whitespaces).isEmpty || working)
                    if row.look == .picked {
                        Button("Reset") { act { try await $0.publishingReset(id: row.id) } }.disabled(working)
                    }
                    if working { ProgressView() }
                }
                .font(Inter.semibold(14)).foregroundStyle(p.accentHi)
                if let note { Text(note.text).font(Inter.regular(12.5)).foregroundStyle(note.failed ? p.dropped : p.dim) }
            }
            .padding(20)
        }
        .background(p.bg)
    }

    private func act(_ f: @escaping (PagesSource) async throws -> Void) {
        guard let src = model.repo as? PagesSource else { return }
        working = true
        Task {
            do {
                try await f(src)
                note = (false, "Saved. The releases come from that series now.")
                changed()
            } catch {
                note = (true, message(of: error))
            }
            working = false
        }
    }
}
