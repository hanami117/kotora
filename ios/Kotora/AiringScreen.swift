import SwiftUI

/// Sample weekly slots. kotora-mobile has no airing API yet (PLAN §1.2 `airing_week`), so the
/// spike keeps made-up slots for the sample shows here; the shows and progress come from the core.
enum SampleAiring {
    struct Slot {
        /// 1 = Sunday … 7 = Saturday (Calendar's weekday).
        let weekday: Int
        let hour: Int
        let minute: Int
        /// Episodes out so far.
        let aired: UInt32

        func next(after now: Date) -> Date {
            Calendar.current.nextDate(after: now, matching: DateComponents(hour: hour, minute: minute, weekday: weekday), matchingPolicy: .nextTime) ?? now
        }

        var shortWhen: String {
            let d = next(after: Date())
            let f = DateFormatter()
            f.locale = Locale(identifier: "en_US_POSIX")
            f.dateFormat = Calendar.current.isDateInToday(d) ? "'Today' HH:mm" : "EEE HH:mm"
            return f.string(from: d)
        }
    }

    static let slots: [UInt32: Slot] = [
        7: Slot(weekday: 5, hour: 23, minute: 0, aired: 7),
        3: Slot(weekday: 7, hour: 1, minute: 30, aired: 6),
        11: Slot(weekday: 4, hour: 22, minute: 0, aired: 3),
        5: Slot(weekday: 2, hour: 20, minute: 0, aired: 2),
        29: Slot(weekday: 4, hour: 23, minute: 30, aired: 4),
    ]
}

struct AiringScreen: View {
    @Environment(\.palette) private var p
    @EnvironmentObject private var model: CoreModel
    @Binding var path: [RowRef]
    @State private var tab = 0

    private struct Item: Identifiable {
        let row: Row
        let slot: SampleAiring.Slot
        let at: Date
        var id: UInt32 { row.id }
    }

    var body: some View {
        let _ = model.version
        let now = Date()
        let items = model.allRows(.anime)
            .compactMap { r in SampleAiring.slots[r.id].map { Item(row: r, slot: $0, at: $0.next(after: now)) } }
            .sorted { $0.at < $1.at }
        let days = Dictionary(grouping: items) { Calendar.current.startOfDay(for: $0.at) }.sorted { $0.key < $1.key }
        let toWatch = items.reduce(0) { $0 + Int($1.slot.aired.subtracting($1.row.progress)) }
        let today = items.filter { Calendar.current.isDateInToday($0.at) }.count

        VStack(spacing: 0) {
            AppBar(title: "Airing") {
                Image(systemName: "bell").font(.system(size: 18)).foregroundStyle(p.dim).frame(width: 40, height: 40)
                Avatar()
            }
            Segmented(items: [(0, "This week"), (1, "Season")], selection: $tab, height: 28).padding(.horizontal, 16)
            HStack(spacing: 10) {
                stat("\(toWatch)", "to watch", accent: true)
                stat("\(today)", "airing today")
                stat("\(items.count)", "shows this week")
            }
            .padding(.horizontal, 16).padding(.top, 12)
            if tab == 0 {
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 0) {
                        ForEach(Array(days.enumerated()), id: \.element.key) { i, day in
                            SectionTitle(text: dayTitle(day.key), trailing: i == 0 ? "sample times" : nil)
                            Card {
                                ForEach(Array(day.value.enumerated()), id: \.element.id) { j, item in
                                    airingRow(item, last: j == day.value.count - 1)
                                }
                            }
                        }
                    }
                    .padding(.bottom, 24)
                }
            } else {
                Spacer()
                Text("The season chart comes with the airing API.").font(Inter.regular(14)).foregroundStyle(p.faint)
                Spacer()
            }
        }
        .background(p.bg)
    }

    private func airingRow(_ item: Item, last: Bool) -> some View {
        let behind = Int(item.slot.aired) - Int(item.row.progress)
        return VStack(spacing: 0) {
            HStack(spacing: 12) {
                CoverTile(id: item.row.id, initials: item.row.initials, width: 44, height: 62)
                VStack(alignment: .leading, spacing: 5) {
                    Text(item.row.title).font(Inter.medium(15)).foregroundStyle(p.text).lineLimit(1)
                    HStack(spacing: 6) {
                        Text(Self.hhmm(item.at))
                            .font(Inter.semibold(13)).monospacedDigit().foregroundStyle(p.air)
                        Text("Episode \(item.slot.aired + 1)").font(Inter.regular(13)).foregroundStyle(p.dim)
                    }
                    HStack(spacing: 6) {
                        if behind > 0 { Tag(text: "\(behind) to watch", style: .new) }
                        Tag(text: item.row.status.label(.anime))
                    }
                }
                Spacer()
            }
            .padding(12)
            .contentShape(Rectangle())
            .onTapGesture { path.append(RowRef(kind: .anime, id: item.row.id)) }
            if !last { Rectangle().fill(p.border).frame(height: 1).padding(.leading, 68) }
        }
    }

    private func stat(_ n: String, _ label: String, accent: Bool = false) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(n).font(Inter.semibold(22)).monospacedDigit().foregroundStyle(accent ? p.accentHi : p.text)
            Text(label).font(Inter.regular(12)).foregroundStyle(p.faint)
        }
        .padding(12).frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 12).fill(p.surface))
        .overlay(RoundedRectangle(cornerRadius: 12).stroke(p.border, lineWidth: 1))
    }

    static func hhmm(_ d: Date) -> String {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.dateFormat = "HH:mm"
        return f.string(from: d)
    }

    private func dayTitle(_ d: Date) -> String {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.dateFormat = "EEE d MMM"
        let s = f.string(from: d)
        if Calendar.current.isDateInToday(d) { return "Today · \(s)" }
        if Calendar.current.isDateInTomorrow(d) { return "Tomorrow · \(s)" }
        return s
    }
}

private extension UInt32 {
    func subtracting(_ o: UInt32) -> UInt32 { self > o ? self - o : 0 }
}
