import SwiftUI

/// Details from the saved list (as Android M2). The service's own page (synopsis, rank) comes with
/// the details API later; the text below says so instead of making things up.
struct DetailsScreen: View {
    @Environment(\.palette) private var p
    @EnvironmentObject private var model: CoreModel
    let ref: RowRef

    var body: some View {
        let _ = model.version
        Group {
            if let row = model.entry(ref.kind, ref.id) {
                content(row)
            } else {
                Text("That isn't on your list.").font(Inter.regular(15)).foregroundStyle(p.faint)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(p.bg)
        .toolbarBackground(.hidden, for: .navigationBar)
        .overlay(alignment: .bottom) {
            if let snack = model.snack { SnackBar(snack: snack).transition(.move(edge: .bottom).combined(with: .opacity)) }
        }
    }

    private func content(_ row: Row) -> some View {
        let unit = ref.kind == .manga ? "chapter" : "episode"
        let units = ref.kind == .manga ? "Chapters" : "Episodes"
        let slot = ref.kind == .anime ? SampleAiring.slots[row.id] : nil
        return ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                hero(row)
                pills(row, slot: slot).padding(.horizontal, 16).padding(.top, 14)

                SectionTitle(text: "Your list", trailing: "MyAnimeList · sample")
                Card {
                    HStack(spacing: 10) {
                        field("Status") {
                            Circle().fill(p.status(row.status)).frame(width: 8, height: 8)
                            Text(row.status.label(ref.kind))
                        }
                        field("Score") { Text("★ \(row.score ?? "–")") }
                    }
                    .padding(16)
                    VStack(alignment: .leading, spacing: 6) {
                        Text(units).font(Inter.medium(12)).foregroundStyle(p.faint)
                        HStack(spacing: 10) {
                            Text("\(row.progress) of \(row.total.map(String.init) ?? "?")")
                                .font(Inter.semibold(15)).monospacedDigit().foregroundStyle(p.text)
                                .frame(width: 110, height: 44)
                                .background(RoundedRectangle(cornerRadius: 10).fill(p.field))
                                .overlay(RoundedRectangle(cornerRadius: 10).stroke(p.borderStrong, lineWidth: 1))
                            Button { model.plusOne(row) } label: {
                                Text(canPlus(row) ? "+1 \(unit.capitalized) \(row.progress + 1)" : "All \(units.lowercased()) done")
                                    .font(Inter.semibold(15)).foregroundStyle(canPlus(row) ? p.onAccent : p.faint)
                                    .frame(maxWidth: .infinity).frame(height: 44)
                                    .background(RoundedRectangle(cornerRadius: 12).fill(canPlus(row) ? p.accent : p.raised))
                            }
                            .disabled(!canPlus(row))
                        }
                    }
                    .padding(.horizontal, 16).padding(.bottom, 14)
                    if row.rewatching || row.timesRewatched > 0 {
                        Rectangle().fill(p.border).frame(height: 1)
                        ListLine(title: ref.kind == .manga ? "Rereading" : "Rewatching", subtitle: row.timesRewatched > 0 ? "\(row.timesRewatched) times before" : nil, divider: false) {
                            EmptyView()
                        } trailing: {
                            Image(systemName: row.rewatching ? "checkmark.circle.fill" : "circle").foregroundStyle(row.rewatching ? p.accent : p.faint)
                        }
                    }
                    if row.waiting {
                        Rectangle().fill(p.border).frame(height: 1)
                        ListLine(title: "Waiting to sync", subtitle: "Saved here; a signed-in build would send it", divider: false) {
                            Image(systemName: "clock.arrow.circlepath").foregroundStyle(p.onHold)
                        } trailing: { EmptyView() }
                    }
                }

                if let slot {
                    SectionTitle(text: "Next episode")
                    Card {
                        ListLine(title: "Episode \(slot.aired + 1) · \(slot.shortWhen)", subtitle: "Sample time; estimated from the weekly slot", divider: false) {
                            Image(systemName: "clock").foregroundStyle(p.air)
                        } trailing: {
                            Image(systemName: "bell").foregroundStyle(p.accentHi)
                        }
                    }
                }

                SectionTitle(text: "About")
                Text("Synopsis, rank and studio come from the service's details page, which the iOS spike does not fetch. Everything here is made up.")
                    .font(Inter.regular(14)).foregroundStyle(p.dim).lineSpacing(3)
                    .padding(.horizontal, 20)
                VStack(alignment: .leading, spacing: 8) {
                    kv("Format", row.formatLabel)
                    kv(ref.kind == .manga ? "Published" : "Aired", row.startDate.map(Self.month) ?? "?")
                    if ref.kind == .manga { kv("Volumes", "\(row.volumes) of \(row.totalVolumes.map(String.init) ?? "?")") }
                }
                .padding(.horizontal, 20).padding(.top, 14).padding(.bottom, 80)
            }
        }
        .ignoresSafeArea(edges: .top)
    }

    private func hero(_ row: Row) -> some View {
        ZStack(alignment: .bottomLeading) {
            CoverTile(id: row.id, initials: "", width: nil, height: 230, radius: 0)
                .blur(radius: 24).opacity(0.85)
                .clipped()
                .overlay(LinearGradient(colors: [p.bg.opacity(0.1), p.bg], startPoint: .top, endPoint: .bottom))
            HStack(alignment: .bottom, spacing: 14) {
                CoverTile(id: row.id, initials: row.initials, width: 104, height: 148, radius: 12)
                    .shadow(color: .black.opacity(0.35), radius: 10, y: 4)
                VStack(alignment: .leading, spacing: 4) {
                    Text(row.title).font(Inter.semibold(22)).foregroundStyle(p.text).fixedSize(horizontal: false, vertical: true)
                    if let en = row.titleEn { Text(en).font(Inter.regular(13.5)).foregroundStyle(p.dim) }
                    Text(meta(row)).font(Inter.regular(13)).foregroundStyle(p.faint)
                }
                .padding(.bottom, 4)
            }
            .padding(.horizontal, 16).offset(y: 60)
        }
        .frame(height: 230)
        .padding(.bottom, 60)
    }

    private func pills(_ row: Row, slot: SampleAiring.Slot?) -> some View {
        HStack(spacing: 8) {
            pill("★ \(row.score ?? "–")", color: p.text)
            if row.airing { pill(ref.kind == .manga ? "● Publishing" : "● Airing", color: p.air) }
            pill(row.formatLabel, color: p.dim)
        }
    }

    private func pill(_ text: String, color: Color) -> some View {
        Text(text).font(Inter.medium(13)).foregroundStyle(color)
            .padding(.horizontal, 11).frame(height: 30)
            .overlay(Capsule().stroke(color == p.air ? p.air.opacity(0.4) : p.borderStrong, lineWidth: 1))
    }

    private func field<C: View>(_ label: String, @ViewBuilder _ c: () -> C) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(label).font(Inter.medium(12)).foregroundStyle(p.faint)
            HStack(spacing: 8) { c() }
                .font(Inter.medium(15)).foregroundStyle(p.text)
                .padding(.horizontal, 12).frame(maxWidth: .infinity, alignment: .leading).frame(height: 44)
                .background(RoundedRectangle(cornerRadius: 10).fill(p.field))
                .overlay(RoundedRectangle(cornerRadius: 10).stroke(p.borderStrong, lineWidth: 1))
        }
    }

    private func kv(_ k: String, _ v: String) -> some View {
        HStack {
            Text(k).font(Inter.regular(14)).foregroundStyle(p.faint).frame(width: 96, alignment: .leading)
            Text(v).font(Inter.regular(14)).foregroundStyle(p.text)
        }
    }

    private func canPlus(_ row: Row) -> Bool { row.total.map { row.progress < $0 } ?? true }

    private func meta(_ row: Row) -> String {
        let n = row.total.map { "\($0) \(ref.kind == .manga ? "chapters" : "episodes")" } ?? "? \(ref.kind == .manga ? "chapters" : "episodes")"
        return "\(row.formatLabel) · \(n)"
    }

    static func month(_ iso: String) -> String {
        let f = DateFormatter()
        f.dateFormat = "yyyy-MM-dd"
        guard let d = f.date(from: iso) else { return iso }
        f.dateFormat = "MMM yyyy"
        return f.string(from: d)
    }
}
