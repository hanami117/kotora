import SwiftUI

/// Airing, as on the desktop: the week (the next 7 days, by the estimated broadcast time in the
/// phone's zone) and the season's anime, from the active service's season lists (the current
/// season and the one before, kept for 12 hours), merged with the list's own airing shows.
/// Pull down to ask again. The texts come from `kotora-mobile` in the desktop's words.
struct AiringScreen: View {
    @Environment(\.palette) private var p
    @EnvironmentObject private var model: AppModel
    @Binding var path: [RowRef]

    enum View_: Hashable { case week, season }
    @State private var view: View_ = .week
    @State private var type: AiringType = .all
    @State private var mine: AiringMine = .all
    @State private var order: AiringOrder = .popular
    @State private var page: AiringPage?
    @State private var loading = false
    @State private var error: String?
    @State private var noSource = false

    var body: some View {
        VStack(spacing: 0) {
            AppBar(title: "Airing") {
                if loading { ProgressView().tint(p.dim).frame(width: 40, height: 40) }
                else {
                    Button { Task { await refresh(force: true) } } label: {
                        Image(systemName: "arrow.triangle.2.circlepath").font(.system(size: 18)).foregroundStyle(p.dim).frame(width: 40, height: 40)
                    }
                    .accessibilityLabel("Refresh the season")
                }
                AccountMenu()
            }
            Segmented(items: [(View_.week, "This week"), (.season, page?.season ?? "Season")], selection: $view)
                .padding(.horizontal, 16)
            filters
            if let error {
                Banner(text: (page?.hasShows ?? false) ? "\(error) Showing the saved season." : error, retry: { Task { await refresh(force: true) } })
            }
            content
        }
        .background(p.bg)
        .task { await start() }
        .task(id: model.list.version) { await load() }
        .onChange(of: type) { Task { await load() } }
        .onChange(of: mine) { Task { await load() } }
        .onChange(of: order) { Task { await load() } }
        .onReceive(Timer.publish(every: 60, on: .main, in: .common).autoconnect()) { _ in Task { await load() } }
    }

    @ViewBuilder private var content: some View {
        if noSource {
            empty("Airing needs your list", "Sign in to see the seasons.")
        } else if let page, page.hasShows {
            List {
                if view == .week { week(page) } else { season(page) }
                Text("Times are estimated; ~ marks estimated counts").font(Inter.regular(12)).foregroundStyle(p.faint)
                    .frame(maxWidth: .infinity).padding(.vertical, 12)
                    .listRowBackground(p.bg).listRowSeparator(.hidden)
                Color.clear.frame(height: 40).listRowBackground(p.bg).listRowSeparator(.hidden)
            }
            .listStyle(.plain)
            .scrollContentBackground(.hidden)
            .refreshable { await refresh(force: true) }
        } else if loading || page == nil {
            VStack(alignment: .leading, spacing: 8) {
                Text("Loading \(page?.season ?? "the season")…").font(Inter.regular(13)).foregroundStyle(p.faint)
                ForEach(0..<4, id: \.self) { _ in RoundedRectangle(cornerRadius: 10).fill(p.raised).frame(height: 44) }
                Spacer()
            }
            .padding(16)
        } else if let page {
            error != nil
                ? empty("No season to show", "Kotora shows the season once \(page.serviceTitle) answers.")
                : empty("Nothing airing", "\(page.serviceTitle) lists no anime for this season yet.")
        }
    }

    // ------------------------------------------------------------------------------- the week

    @ViewBuilder private func week(_ page: AiringPage) -> some View {
        if page.days.allSatisfy({ $0.slots.isEmpty }) && page.untimed.isEmpty {
            VStack(spacing: 4) {
                Text("Nothing airs this week with these filters").font(Inter.semibold(16)).foregroundStyle(p.text)
                Text("Try Everything, or All types.").font(Inter.regular(13)).foregroundStyle(p.dim)
            }
            .frame(maxWidth: .infinity).padding(.top, 48)
            .listRowBackground(p.bg).listRowSeparator(.hidden)
        } else {
            ForEach(Array(page.days.enumerated()), id: \.offset) { _, day in
                Section {
                    if day.slots.isEmpty {
                        Text("Nothing airs").font(Inter.regular(13)).foregroundStyle(p.faint)
                            .padding(.leading, 62).listRowBackground(p.bg).listRowSeparator(.hidden)
                    }
                    ForEach(day.slots, id: \.id) { slot($0) }
                } header: {
                    dayHead(day.label, today: day.today, end: day.onList)
                }
            }
            if !page.untimed.isEmpty {
                Section {
                    ForEach(page.untimed, id: \.id) { u in
                        line(id: u.id, title: u.title, cover: u.coverUrl, time: "–", status: u.status, mine: false) {
                            Text(u.why).font(Inter.regular(12.5)).foregroundStyle(p.faint).lineLimit(1)
                        }
                    }
                } header: {
                    dayHead("No broadcast time", today: false, end: "airing on \(page.serviceTitle), no time to estimate")
                }
            }
        }
    }

    private func dayHead(_ label: String, today: Bool, end: String) -> some View {
        HStack(spacing: 8) {
            Text(label.uppercased()).font(Inter.semibold(12)).tracking(0.5).foregroundStyle(today ? p.accentHi : p.faint)
            if !end.isEmpty { Text(end).font(Inter.regular(12)).foregroundStyle(p.faint).lineLimit(1) }
            if today { Circle().fill(p.accent).frame(width: 6, height: 6) }
            Spacer()
        }
        .padding(.top, 10)
        .listRowInsets(EdgeInsets(top: 0, leading: 16, bottom: 4, trailing: 16))
    }

    private func slot(_ s: AiringSlot) -> some View {
        line(id: s.id, title: s.title, cover: s.coverUrl, time: s.time, status: s.status, mine: s.status != nil) {
            HStack(spacing: 6) {
                Text("Ep \(s.episode)").font(Inter.medium(12.5)).monospacedDigit().foregroundStyle(p.dim)
                if !s.note.isEmpty {
                    Text(s.note).font(Inter.medium(12.5)).monospacedDigit().foregroundStyle(s.note == "aired" ? p.faint : p.accentHi)
                }
                if let w = s.toWatch {
                    Text("·").foregroundStyle(p.faint)
                    Text(w).font(Inter.medium(12.5)).monospacedDigit().foregroundStyle(w.contains("to watch") ? p.text : p.faint)
                }
            }
            .lineLimit(1)
        }
    }

    /// A line of the week: time, cover, title and what's under it, the list status.
    private func line<Under: View>(id: UInt32, title: String, cover: String?, time: String, status: ListStatus?, mine: Bool,
                                   @ViewBuilder under: () -> Under) -> some View {
        HStack(spacing: 12) {
            Text(time).font(Inter.semibold(14)).monospacedDigit().foregroundStyle(mine ? p.text : p.dim).frame(width: 46, alignment: .leading)
            CoverTile(id: id, initials: initials(title), url: cover, width: 30, height: 42, radius: 4)
            VStack(alignment: .leading, spacing: 3) {
                Text(title).font(Inter.medium(14.5)).foregroundStyle(mine ? p.text : p.dim).lineLimit(1)
                under()
            }
            Spacer(minLength: 4)
            if let status { StatusTag(status: status) }
        }
        .padding(.vertical, 6)
        .contentShape(Rectangle())
        .onTapGesture { if status != nil { path.append(RowRef(kind: .anime, id: id)) } }
        .listRowInsets(EdgeInsets(top: 0, leading: 16, bottom: 0, trailing: 12))
        .listRowBackground(mine ? p.accent.opacity(0.06) : p.bg)
        .listRowSeparatorTint(p.border)
        .overlay(alignment: .leading) { if mine { Capsule().fill(p.accent).frame(width: 3).padding(.vertical, 8).offset(x: -16) } }
    }

    // ----------------------------------------------------------------------------- the season

    @ViewBuilder private func season(_ page: AiringPage) -> some View {
        ForEach(page.cards, id: \.id) { c in
            HStack(alignment: .top, spacing: 12) {
                CoverTile(id: c.id, initials: initials(c.title), url: c.coverUrl, width: 52, height: 74, radius: 6)
                VStack(alignment: .leading, spacing: 3) {
                    Text(c.title).font(Inter.medium(15)).foregroundStyle(p.text).lineLimit(2)
                    Text(c.meta).font(Inter.regular(12.5)).foregroundStyle(p.dim)
                    if !c.when.isEmpty {
                        Text(c.when).font(Inter.medium(12.5)).monospacedDigit().foregroundStyle(c.lit ? p.accentHi : p.faint)
                    }
                    if let w = c.toWatch { Text(w).font(Inter.medium(12.5)).foregroundStyle(w.contains("to watch") ? p.text : p.faint) }
                }
                Spacer(minLength: 4)
                if let s = c.status { StatusTag(status: s) }
            }
            .padding(.vertical, 8)
            .contentShape(Rectangle())
            .onTapGesture { if c.status != nil { path.append(RowRef(kind: .anime, id: c.id)) } }
            .listRowInsets(EdgeInsets(top: 0, leading: 16, bottom: 0, trailing: 12))
            .listRowBackground(p.bg)
            .listRowSeparatorTint(p.border)
        }
    }

    // ---------------------------------------------------------------------------------- parts

    private var filters: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 6) {
                ForEach([(AiringType.all, "All types"), (.tv, "TV"), (.ona, "ONA"), (.movie, "Movie")], id: \.0) { t in
                    Chip(text: t.1, on: type == t.0) { type = t.0 }
                }
                Rectangle().fill(p.border).frame(width: 1, height: 18).padding(.horizontal, 4)
                ForEach([(AiringMine.all, "Everything"), (.on, "On my list"), (.off, "Not on my list")], id: \.0) { m in
                    Chip(text: m.1, on: mine == m.0) { mine = m.0 }
                }
                if view == .season {
                    Rectangle().fill(p.border).frame(width: 1, height: 18).padding(.horizontal, 4)
                    ForEach([(AiringOrder.popular, "Most popular"), (.toWatch, "Most to watch")], id: \.0) { o in
                        Chip(text: o.1, on: order == o.0) { order = o.0 }
                    }
                }
            }
            .padding(.horizontal, 16).padding(.vertical, 10)
        }
    }

    private func empty(_ title: String, _ help: String) -> some View {
        VStack(spacing: 4) {
            Spacer()
            Text(title).font(Inter.semibold(17)).foregroundStyle(p.text)
            Text(help).font(Inter.regular(13.5)).foregroundStyle(p.dim).multilineTextAlignment(.center)
            Spacer()
        }
        .padding(.horizontal, 32).frame(maxWidth: .infinity)
    }

    private func initials(_ t: String) -> String {
        String(t.split(separator: " ").prefix(2).compactMap(\.first)).uppercased()
    }

    // ----------------------------------------------------------------------------------- data

    private func start() async {
        guard model.repo as? PagesSource != nil else { noSource = true; return }
        await load()
        Task { await Notifier.askPermission() }
        if page?.stale ?? true { await refresh(force: false) }
    }

    private func load() async {
        guard let src = model.repo as? PagesSource else { return }
        do {
            page = try await src.airingPage(type: type, mine: mine, order: order)
        } catch KotoraError.NotSignedIn {
            noSource = true
        } catch {
            self.error = message(of: error)
        }
    }

    private func refresh(force: Bool) async {
        guard let src = model.repo as? PagesSource, !loading else { return }
        loading = true
        defer { loading = false }
        do {
            try await src.airingRefresh(force: force)
            error = nil
        } catch {
            let svc = page?.serviceTitle ?? "the service"
            if case KotoraError.Offline = error, !(page?.hasShows ?? false) {
                self.error = "Offline: the season lists come from \(svc)."
            } else {
                self.error = message(of: error)
            }
        }
        await load()
    }
}

/// A filter chip, as the list's status chips.
struct Chip: View {
    @Environment(\.palette) private var p
    let text: String
    let on: Bool
    let action: () -> Void

    var body: some View {
        Text(text).font(Inter.medium(13)).foregroundStyle(on ? p.text : p.dim)
            .padding(.horizontal, 12).frame(height: 30)
            .background(Capsule().fill(on ? p.tint : .clear))
            .overlay(Capsule().stroke(on ? p.accent : p.borderStrong, lineWidth: 1))
            .contentShape(Capsule())
            .onTapGesture(perform: action)
            .accessibilityAddTraits(on ? [.isButton, .isSelected] : .isButton)
    }
}

/// The list status of an anime (or manga) on a page that isn't the list.
struct StatusTag: View {
    @Environment(\.palette) private var p
    let status: ListStatus
    var kind: MediaKind = .anime

    var body: some View {
        Text(status.label(kind)).font(Inter.medium(11.5)).foregroundStyle(p.status(status))
            .padding(.horizontal, 7).frame(height: 20)
            .background(RoundedRectangle(cornerRadius: 6).fill(p.status(status).opacity(0.14)))
    }
}

/// An error line with Retry, at the top of a page.
struct Banner: View {
    @Environment(\.palette) private var p
    let text: String
    var retry: (() -> Void)?

    var body: some View {
        HStack(spacing: 10) {
            Circle().fill(p.dropped).frame(width: 8, height: 8)
            Text(text).font(Inter.regular(13)).foregroundStyle(p.text).fixedSize(horizontal: false, vertical: true)
            Spacer(minLength: 4)
            if let retry { Button("Retry", action: retry).font(Inter.semibold(13)).foregroundStyle(p.accentHi) }
        }
        .padding(12)
        .background(RoundedRectangle(cornerRadius: 10).fill(p.dropped.opacity(0.10)))
        .padding(.horizontal, 16).padding(.bottom, 8)
    }
}
