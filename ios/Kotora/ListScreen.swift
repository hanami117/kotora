import SwiftUI

/// My list: Anime | Manga, the status chips with counts, the sync line with the sort, the rows
/// with +1. Pull down to sync (the core's sync, after sending the waiting changes).
struct ListScreen: View {
    @Environment(\.palette) private var p
    @EnvironmentObject private var model: AppModel
    @Binding var path: [RowRef]

    var body: some View {
        let ui = model.list
        VStack(spacing: 0) {
            AppBar(title: "My list") {
                Button { model.sync() } label: {
                    Group {
                        if ui.syncing { ProgressView().tint(p.dim) } else { Image(systemName: "arrow.triangle.2.circlepath").font(.system(size: 18)) }
                    }
                    .foregroundStyle(p.dim).frame(width: 40, height: 40)
                }
                .disabled(ui.syncing)
                .accessibilityLabel("Sync now")
                AccountMenu()
            }
            Segmented(items: [(MediaKind.anime, "Anime"), (.manga, "Manga")],
                      selection: Binding(get: { ui.kind }, set: { model.select(kind: $0) }))
                .padding(.horizontal, 16)
            chips(ui)
            syncLine(ui)
            SignInAgainBanner()
            filterBox(ui)
            if ui.query.trimmingCharacters(in: .whitespaces).isEmpty { quickChips(ui) }
            if ui.view == .list {
                List {
                    ForEach(ui.lines, id: \.row.key) { line in
                        RowView(row: line.row, line: line) { model.plusOne(line.row) }
                            .contentShape(Rectangle())
                            .onTapGesture { path.append(RowRef(kind: line.row.kind, id: line.row.id)) }
                            .contextMenu { OpenOnMenuItems(kind: line.row.kind, id: line.row.id) }
                            .listRowInsets(EdgeInsets(top: 0, leading: 16, bottom: 0, trailing: 12))
                            .listRowBackground(p.bg)
                            .listRowSeparatorTint(p.border)
                    }
                    if ui.loaded && ui.lines.isEmpty {
                        emptyState(ui).listRowBackground(p.bg).listRowSeparator(.hidden)
                    }
                    Color.clear.frame(height: 60).listRowBackground(p.bg).listRowSeparator(.hidden)
                }
                .listStyle(.plain)
                .scrollContentBackground(.hidden)
                .refreshable { await model.refresh() }
            } else {
                ScrollView {
                    PosterGrid(lines: ui.lines, large: ui.view == .large,
                               open: { path.append(RowRef(kind: $0.kind, id: $0.id)) },
                               plusOne: { model.plusOne($0) })
                    if ui.loaded && ui.lines.isEmpty { emptyState(ui) }
                    Color.clear.frame(height: 60)
                }
                .refreshable { await model.refresh() }
            }
        }
        .background(p.bg)
    }

    private func chips(_ ui: ListUi) -> some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(ListStatus.allCases, id: \.self) { st in
                    let on = st == ui.status
                    HStack(spacing: 7) {
                        Circle().fill(p.status(st)).frame(width: 8, height: 8)
                        Text(st.label(ui.kind)).font(Inter.medium(13)).foregroundStyle(on ? p.text : p.dim)
                        Text("\(ui.count(st))").font(Inter.regular(13)).monospacedDigit().foregroundStyle(p.faint)
                    }
                    .padding(.horizontal, 13).frame(height: 34)
                    .background(Capsule().fill(on ? p.tint : .clear))
                    .overlay(Capsule().stroke(on ? p.accent : p.borderStrong, lineWidth: 1))
                    .contentShape(Capsule())
                    .onTapGesture { model.select(status: st) }
                    .accessibilityAddTraits(on ? [.isButton, .isSelected] : .isButton)
                }
            }
            .padding(.horizontal, 16).padding(.top, 12).padding(.bottom, 4)
        }
    }

    /// The Sort menu (desktop `sort_menu`): "Sort by" the four orders, then "Order" the two directions.
    private func sortMenu(_ ui: ListUi) -> some View {
        Menu {
            Section("Sort by") {
                Picker("Sort by", selection: Binding(get: { ui.sort }, set: { model.select(sort: $0) })) {
                    ForEach(ListSort.allCases, id: \.self) { Text($0.label).tag($0) }
                }
            }
            Section("Order") {
                Picker("Order", selection: Binding(get: { ui.reverse }, set: { model.select(reverse: $0) })) {
                    ForEach(Array(ui.sort.directions.enumerated()), id: \.offset) { i, d in Text(d).tag(i == 1) }
                }
            }
        } label: {
            HStack(spacing: 4) {
                Text("Sort: \(ui.sort.label)").font(Inter.regular(12))
                Image(systemName: ui.reverse ? "arrow.up" : "arrow.down").font(.system(size: 10, weight: .semibold))
            }
            .foregroundStyle(p.dim).padding(.vertical, 6)
        }
        .accessibilityLabel("Sort: \(ui.sort.label), \(ui.sort.directions[ui.reverse ? 1 : 0])")
    }

    /// "Filter your list": the whole list's matches while it holds text (desktop ⌘F).
    private func filterBox(_ ui: ListUi) -> some View {
        HStack(spacing: 8) {
            Image(systemName: "magnifyingglass").font(.system(size: 14)).foregroundStyle(p.faint)
            TextField("", text: Binding(get: { ui.query }, set: { model.select(query: $0) }),
                      prompt: Text(listFilterHint(kind: ui.kind)).foregroundColor(p.faint))
                .font(Inter.regular(14)).foregroundStyle(p.text)
                .textInputAutocapitalization(.never).autocorrectionDisabled().submitLabel(.search)
            if !ui.query.isEmpty {
                Button { model.select(query: "") } label: {
                    Image(systemName: "xmark.circle.fill").font(.system(size: 15)).foregroundStyle(p.faint)
                }
                .buttonStyle(.borderless).accessibilityLabel("Clear filter")
            }
        }
        .padding(.horizontal, 12).frame(height: 38)
        .background(RoundedRectangle(cornerRadius: 10).fill(p.field))
        .overlay(RoundedRectangle(cornerRadius: 10).stroke(p.border, lineWidth: 1))
        .padding(.horizontal, 16).padding(.top, 6).padding(.bottom, 2)
    }

    /// The chips above the list (All / Airing / Finished airing / New episode; manga Publishing /
    /// Finished / On hiatus) and, at the end, the view switch.
    private func quickChips(_ ui: ListUi) -> some View {
        HStack(spacing: 6) {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 6) {
                    ForEach(quickFilters(kind: ui.kind), id: \.self) { f in
                        let on = f == ui.filter
                        Text(f.label).font(Inter.medium(12.5)).foregroundStyle(on ? p.text : p.dim)
                            .padding(.horizontal, 11).frame(height: 28)
                            .background(Capsule().fill(on ? p.raised : .clear))
                            .overlay(Capsule().stroke(on ? p.borderStrong : p.border, lineWidth: 1))
                            .contentShape(Capsule())
                            .onTapGesture { model.select(filter: f) }
                            .accessibilityAddTraits(on ? [.isButton, .isSelected] : .isButton)
                    }
                }
                .padding(.leading, 16)
            }
            HStack(spacing: 0) {
                ForEach(ListViewMode.allCases, id: \.self) { v in
                    let on = v == ui.view
                    Button { model.setView(v) } label: {
                        Image(systemName: v.icon).font(.system(size: 13, weight: .medium))
                            .foregroundStyle(on ? p.text : p.faint)
                            .frame(width: 32, height: 28)
                            .background(RoundedRectangle(cornerRadius: 6).fill(on ? p.raised : .clear))
                    }
                    .buttonStyle(.borderless)
                    .accessibilityLabel("\(v.label) view")
                    .accessibilityAddTraits(on ? .isSelected : [])
                }
            }
            .padding(2)
            .overlay(RoundedRectangle(cornerRadius: 8).stroke(p.border, lineWidth: 1))
            .padding(.trailing, 14)
        }
        .padding(.top, 6).padding(.bottom, 4)
    }

    /// No lines: loading, the status is empty (desktop `empty`), or nothing matches (`no_match`).
    private func emptyState(_ ui: ListUi) -> some View {
        VStack(spacing: 4) {
            if ui.empty == .loading {
                Text(ui.emptyTitle)
                    .font(Inter.regular(14)).foregroundStyle(p.faint)
            } else {
                Text(ui.emptyTitle).font(Inter.semibold(16)).foregroundStyle(p.text)
                if !ui.emptyHelp.isEmpty { Text(ui.emptyHelp).font(Inter.regular(14)).foregroundStyle(p.dim) }
            }
        }
        .multilineTextAlignment(.center)
        .frame(maxWidth: .infinity).padding(.top, 48).padding(.horizontal, 16)
    }

    private func syncLine(_ ui: ListUi) -> some View {
        let service = model.session.active?.service.label ?? ""
        let (dot, text): (Color, String) =
            if let problem = ui.problem { (p.onHold, problem) }
            else if ui.syncing { (p.faint, "Syncing with \(service)…") }
            else if let at = ui.syncedAt { (p.watching, "Synced \(ago(at, nowSeconds())) · \(service)") }
            else { (p.faint, "Not synced yet · \(service)") }
        return HStack(spacing: 6) {
            Circle().fill(dot).frame(width: 8, height: 8)
            Text(text).font(Inter.regular(12)).foregroundStyle(p.faint).lineLimit(1).truncationMode(.tail)
            Spacer(minLength: 4)
            sortMenu(ui)
        }
        .padding(.leading, 16).padding(.trailing, 14).padding(.top, 4).padding(.bottom, 2)
    }
}

struct RowView: View {
    @Environment(\.palette) private var p
    let row: Row
    /// The list page's line: "Ep N out" / "On hiatus" tags and the score in the user's format.
    var line: PageLine? = nil
    let plusOne: () -> Void

    var body: some View {
        HStack(spacing: 12) {
            CoverTile(id: row.id, initials: row.initials, url: row.coverUrl, width: 44, height: 64, radius: 5)
            VStack(alignment: .leading, spacing: 6) {
                Text(row.title).font(Inter.medium(15)).foregroundStyle(p.text).lineLimit(1)
                let tags = line?.tags ?? row.tags
                ViewThatFits(in: .horizontal) {
                    HStack(spacing: 6) { ForEach(tags, id: \.0) { Tag(text: $0.0, style: $0.1) } }
                    HStack(spacing: 6) { ForEach(tags.prefix(2), id: \.0) { Tag(text: $0.0, style: $0.1) } }
                }
                ProgressLine(row: row, mark: line?.scoreMark, hasMark: line != nil)
            }
            Spacer(minLength: 4)
            if row.canPlusOne {
                Button(action: plusOne) {
                    Text("+1").font(Inter.semibold(15)).foregroundStyle(p.text)
                        .frame(width: 56, height: 40)
                        .background(RoundedRectangle(cornerRadius: 8).fill(p.raised))
                        .overlay(RoundedRectangle(cornerRadius: 8).stroke(p.borderStrong, lineWidth: 1))
                }
                .buttonStyle(.borderless)
                .accessibilityLabel("Plus one \(row.kind.unit): \(row.title)")
            }
        }
        .frame(minHeight: 84)
    }
}
