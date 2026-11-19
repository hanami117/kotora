import SwiftUI

struct ListScreen: View {
    @Environment(\.palette) private var p
    @EnvironmentObject private var model: CoreModel
    @Binding var path: [RowRef]

    var body: some View {
        VStack(spacing: 0) {
            AppBar(title: "My list") {
                Button { model.pretendSync() } label: {
                    Image(systemName: "arrow.triangle.2.circlepath").font(.system(size: 18)).foregroundStyle(p.dim).frame(width: 40, height: 40)
                }
                Avatar()
            }
            if model.mangaEnabled {
                Segmented(items: [(MediaKind.anime, "Anime"), (.manga, "Manga")], selection: $model.kind)
                    .padding(.horizontal, 16)
            }
            chips
            syncLine
            List {
                ForEach(model.rows, id: \.key) { row in
                    RowView(row: row, unit: model.unit) { model.plusOne(row) }
                        .contentShape(Rectangle())
                        .onTapGesture { path.append(RowRef(kind: row.kind, id: row.id)) }
                        .listRowInsets(EdgeInsets(top: 0, leading: 16, bottom: 0, trailing: 12))
                        .listRowBackground(p.bg)
                        .listRowSeparatorTint(p.border)
                }
                if model.rows.isEmpty {
                    Text("Nothing here").font(Inter.regular(14)).foregroundStyle(p.faint)
                        .frame(maxWidth: .infinity).padding(.top, 40).listRowBackground(p.bg).listRowSeparator(.hidden)
                }
            }
            .listStyle(.plain)
            .scrollContentBackground(.hidden)
            .refreshable { model.pretendSync() }
        }
        .background(p.bg)
        .onChange(of: model.kind) { model.reload() }
        .onChange(of: model.status) { model.reload() }
        .onChange(of: model.sort) { model.reload() }
    }

    private var chips: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(ListStatus.allCases, id: \.self) { st in
                    let on = st == model.status
                    HStack(spacing: 7) {
                        Circle().fill(p.status(st)).frame(width: 8, height: 8)
                        Text(st.label(model.kind)).font(Inter.medium(13)).foregroundStyle(on ? p.text : p.dim)
                        Text("\(model.count(st))").font(Inter.regular(13)).monospacedDigit().foregroundStyle(p.faint)
                    }
                    .padding(.horizontal, 13).frame(height: 34)
                    .background(Capsule().fill(on ? p.tint : .clear))
                    .overlay(Capsule().stroke(on ? p.accent : p.borderStrong, lineWidth: 1))
                    .contentShape(Capsule())
                    .onTapGesture { model.status = st }
                }
            }
            .padding(.horizontal, 16).padding(.top, 12).padding(.bottom, 4)
        }
    }

    private var syncLine: some View {
        HStack(spacing: 6) {
            Circle().fill(p.watching).frame(width: 7, height: 7)
            Group {
                if let at = model.syncedAt {
                    Text("Sample list · synced \(Self.ago(at))")
                } else {
                    Text("Sample list")
                }
            }
            .font(Inter.regular(12.5)).foregroundStyle(p.faint).lineLimit(1)
            Spacer()
            Menu {
                Picker("Sort", selection: $model.sort) {
                    Text("Updated").tag(Sort.updated)
                    Text("Title").tag(Sort.title)
                    Text("Score").tag(Sort.score)
                }
            } label: {
                HStack(spacing: 3) {
                    Text("Sort: \(sortLabel)").font(Inter.regular(12.5))
                    Image(systemName: "chevron.down").font(.system(size: 10, weight: .semibold))
                }
                .foregroundStyle(p.dim)
            }
        }
        .padding(.horizontal, 20).padding(.vertical, 8)
    }

    static func ago(_ d: Date) -> String {
        let f = RelativeDateTimeFormatter()
        f.unitsStyle = .short
        return f.localizedString(for: d, relativeTo: Date())
    }

    private var sortLabel: String {
        switch model.sort {
        case .updated: return "Updated"
        case .title: return "Title"
        case .score: return "Score"
        }
    }
}

struct RowView: View {
    @Environment(\.palette) private var p
    let row: Row
    let unit: String
    let plusOne: () -> Void

    var body: some View {
        HStack(spacing: 12) {
            CoverTile(id: row.id, initials: row.initials)
            VStack(alignment: .leading, spacing: 6) {
                Text(row.title).font(Inter.medium(15)).foregroundStyle(p.text).lineLimit(1)
                ViewThatFits(in: .horizontal) {
                    HStack(spacing: 6) { tags(short: false) }
                    HStack(spacing: 6) { tags(short: true) }
                }
                ProgressLine(row: row)
            }
            Spacer(minLength: 4)
            if canPlus {
                Button(action: plusOne) {
                    Text("+1").font(Inter.semibold(15))
                        .foregroundStyle(hot ? p.onAccent : p.accentHi)
                        .frame(width: 48, height: 40)
                        .background(RoundedRectangle(cornerRadius: 12).fill(hot ? p.accent : p.tint))
                }
                .buttonStyle(.borderless)
                .accessibilityLabel("Plus one \(unit)")
            }
        }
        .padding(.vertical, 10)
    }

    private var canPlus: Bool {
        (row.status == .watching || row.rewatching) && (row.total.map { row.progress < $0 } ?? true)
    }

    /// Mockup: the +1 glows when a new episode is out.
    private var hot: Bool {
        guard let slot = SampleAiring.slots[row.id], row.kind == .anime else { return false }
        return slot.aired > row.progress
    }

    @ViewBuilder private func tags(short: Bool) -> some View {
        if row.kind == .anime, let slot = SampleAiring.slots[row.id] {
            if slot.aired > row.progress { Tag(text: "Ep \(slot.aired) out", style: .new) }
            Tag(text: short ? slot.shortWhen : "Airing · \(slot.shortWhen)", style: .air)
        } else if row.rewatching {
            Tag(text: row.kind == .manga ? "Rereading" : "Rewatching")
        } else if row.kind == .manga {
            Tag(text: row.volumes > 0 ? "Vol \(row.volumes)" : "Manga")
        } else {
            Tag(text: row.total.map { "\(row.formatLabel) · \($0) eps" } ?? row.formatLabel)
        }
        if row.waiting { Tag(text: short ? "Waiting" : "Waiting to sync", style: .wait) }
    }
}

struct RowRef: Hashable {
    let kind: MediaKind
    let id: UInt32
}
