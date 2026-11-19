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
            List {
                ForEach(ui.rows, id: \.key) { row in
                    RowView(row: row) { model.plusOne(row) }
                        .contentShape(Rectangle())
                        .onTapGesture { path.append(RowRef(kind: row.kind, id: row.id)) }
                        .contextMenu { OpenOnMenuItems(kind: row.kind, id: row.id) }
                        .listRowInsets(EdgeInsets(top: 0, leading: 16, bottom: 0, trailing: 12))
                        .listRowBackground(p.bg)
                        .listRowSeparatorTint(p.border)
                }
                if ui.loaded && ui.rows.isEmpty {
                    Text(ui.syncedAt == nil ? "Your list will show here after the first sync. Pull down to sync now."
                         : "Nothing under \(ui.status.label(ui.kind)).")
                        .font(Inter.regular(14)).foregroundStyle(p.faint).multilineTextAlignment(.center)
                        .frame(maxWidth: .infinity).padding(.top, 48).padding(.horizontal, 16)
                        .listRowBackground(p.bg).listRowSeparator(.hidden)
                }
                Color.clear.frame(height: 60).listRowBackground(p.bg).listRowSeparator(.hidden)
            }
            .listStyle(.plain)
            .scrollContentBackground(.hidden)
            .refreshable { await model.refresh() }
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
            Menu {
                Picker("Sort", selection: Binding(get: { ui.sort }, set: { model.select(sort: $0) })) {
                    ForEach(Sort.allCases, id: \.self) { Text($0.label).tag($0) }
                }
            } label: {
                HStack(spacing: 4) {
                    Text("Sort: \(ui.sort.label)").font(Inter.regular(12))
                    Image(systemName: "chevron.down").font(.system(size: 10, weight: .semibold))
                }
                .foregroundStyle(p.dim).padding(.vertical, 6)
            }
        }
        .padding(.leading, 16).padding(.trailing, 14).padding(.top, 4).padding(.bottom, 2)
    }
}

struct RowView: View {
    @Environment(\.palette) private var p
    let row: Row
    let plusOne: () -> Void

    var body: some View {
        HStack(spacing: 12) {
            CoverTile(id: row.id, initials: row.initials, url: row.coverUrl, width: 44, height: 64, radius: 5)
            VStack(alignment: .leading, spacing: 6) {
                Text(row.title).font(Inter.medium(15)).foregroundStyle(p.text).lineLimit(1)
                ViewThatFits(in: .horizontal) {
                    HStack(spacing: 6) { ForEach(row.tags, id: \.0) { Tag(text: $0.0, style: $0.1) } }
                    HStack(spacing: 6) { ForEach(row.tags.prefix(2), id: \.0) { Tag(text: $0.0, style: $0.1) } }
                }
                ProgressLine(row: row)
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
