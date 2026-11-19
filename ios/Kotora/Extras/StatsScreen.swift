import SwiftUI

/// The Statistics page, the desktop's (`app/src/stats.rs`) at a phone's width: totals, this week /
/// month / year and the streak, the activity calendar, when you watch, most watched lately, your
/// list (statuses and scores), genres and studios (authors), and the airing shows you follow.
/// Follows the list's Anime | Manga switch. Local numbers (the list and Kotora's activity log): no
/// request, read when the page opens, the switch changes or the list changes.
struct StatsScreen: View {
    @Environment(\.palette) private var p
    @EnvironmentObject private var model: AppModel
    @State private var kind: MediaKind = .anime
    @State private var data: StatsData?
    @State private var error: String?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                if model.mangaEnabled {
                    Segmented(items: [(MediaKind.anime, "Anime"), (.manga, "Manga")], selection: $kind)
                }
                if let d = data, d.kind == kind {
                    StatsContent(st: d, titleOf: title(of:))
                } else {
                    Text(error ?? "Counting…").font(Inter.regular(15)).foregroundStyle(p.dim)
                        .frame(maxWidth: .infinity).padding(.vertical, 40)
                }
            }
            .padding(.horizontal, 16).padding(.top, 4).padding(.bottom, 24)
        }
        .background(p.bg)
        .navigationTitle("\(kind == .manga ? "Manga" : "Anime") statistics")
        .navigationBarTitleDisplayMode(.inline)
        .onAppear { kind = model.list.kind }
        .task(id: "\(kind)-\(model.list.version)") { await load() }
    }

    private func load() async {
        guard let src = model.repo as? InsightsSource else { return }
        do {
            data = try await src.stats(kind: kind)
            error = nil
        } catch {
            self.error = "Can't read the statistics: \(message(of: error))"
        }
    }

    /// The title as the list shows it, else the service's main title.
    private func title(of t: StatsTitle) -> String {
        model.list.rows.first { $0.id == t.mediaId && $0.kind == kind }?.title ?? t.title
    }
}

/// The desktop's words for one kind of list.
struct Words {
    let unit, units, verb: String

    init(_ kind: MediaKind) {
        (unit, units, verb) = kind == .anime ? ("episode", "episodes", "watched") : ("chapter", "chapters", "read")
    }

    func count(_ n: UInt64) -> String { n == 1 ? "1 \(unit)" : "\(statsGrouped(n: n)) \(units)" }
}

/// The line under a period's number ("2 h 24 min", "5 vol.", "3 titles", "Nothing watched yet").
func statsPeriodLine(_ kind: MediaKind, _ p: StatsPeriod, _ w: Words) -> String {
    if kind == .anime && p.minutes > 0 { return statsDuration(minutes: p.minutes) }
    if kind == .manga && p.volumes > 0 { return "\(p.volumes) vol." }
    if p.titles == 1 { return "1 title" }
    if p.titles > 0 { return "\(p.titles) titles" }
    return "Nothing \(w.verb) yet"
}

private struct StatsContent: View {
    @Environment(\.palette) private var p
    let st: StatsData
    let titleOf: (StatsTitle) -> String

    var body: some View {
        let w = Words(st.kind)
        VStack(alignment: .leading, spacing: 16) {
            figures(w)
            CalendarCard(st: st, w: w)
            when(w)
            lately(w)
            listCard
            tags
            if st.kind == .anime && !st.airing.isEmpty { airing }
        }
    }

    private var hasActivity: Bool { st.all.units > 0 || st.days.contains { $0 > 0 } }

    private var sinceText: String? { st.sinceDay.map { statsDateText(day: $0) } }

    // ---- the tiles ----

    private func periodLine(_ p: StatsPeriod, _ w: Words) -> String { statsPeriodLine(st.kind, p, w) }

    private func figures(_ w: Words) -> some View {
        var tiles: [(String, String, String)] = []
        tiles.append((statsGrouped(n: st.units), st.kind == .anime ? "Episodes watched" : "Chapters read", "\(statsGrouped(n: UInt64(st.titles))) on your list"))
        if st.kind == .anime {
            tiles.append((statsDuration(minutes: st.minutes), "Time watched", st.guessed > 0 ? "Some lengths guessed" : "From episode lengths"))
        } else {
            tiles.append((statsGrouped(n: st.volumes), "Volumes read", "From your list"))
        }
        for (caption, per) in [("This week", st.week), ("This month", st.month), ("This year", st.year)] {
            tiles.append((statsGrouped(n: UInt64(per.units)), caption, periodLine(per, w)))
        }
        tiles.append((statsDays(n: st.streak), "Streak", st.bestStreak > 0 ? "Best \(statsDays(n: st.bestStreak))" : "Days in a row"))
        return LazyVGrid(columns: [GridItem(.flexible(), spacing: 12), GridItem(.flexible(), spacing: 12)], spacing: 12) {
            ForEach(tiles, id: \.1) { t in
                VStack(alignment: .leading, spacing: 3) {
                    Text(t.1.uppercased()).font(Inter.semibold(11)).tracking(0.4).foregroundStyle(p.faint).lineLimit(1)
                    Text(t.0).font(Inter.semibold(22)).monospacedDigit().foregroundStyle(p.text).lineLimit(1).minimumScaleFactor(0.7)
                    Text(t.2).font(Inter.regular(12.5)).foregroundStyle(p.dim).lineLimit(1).minimumScaleFactor(0.85)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(14)
                .background(RoundedRectangle(cornerRadius: 14).fill(p.surface))
                .overlay(RoundedRectangle(cornerRadius: 14).stroke(p.border, lineWidth: 1))
                .accessibilityElement(children: .combine)
            }
        }
    }

    // ---- when ----

    private func when(_ w: Words) -> some View {
        StatsCard(title: st.kind == .anime ? "When you watch" : "When you read") {
            if !hasActivity {
                emptyLine(w)
            } else {
                let topDay = st.weekdays.indices.max { (st.weekdays[$0], 7 - $0) < (st.weekdays[$1], 7 - $1) } ?? 0
                let topHour = st.hours.indices.max { (st.hours[$0], 24 - $0) < (st.hours[$1], 24 - $1) } ?? 0
                let days = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"]
                Text("Mostly on \(days[topDay])s, around \(String(format: "%02d", topHour)):00.").font(Inter.regular(15)).foregroundStyle(p.dim)
                Caption("By day")
                Bars(values: st.weekdays, labels: ["M", "T", "W", "T", "F", "S", "S"], height: 56, gap: 8)
                Caption("By hour").padding(.top, 4)
                Bars(values: st.hours, labels: (0..<24).map { $0 % 6 == 0 ? "\($0)" : "" }, height: 56, gap: 2)
            }
        }
    }

    private func emptyLine(_ w: Words) -> some View {
        let start = sinceText.map { "Kotora counts from \($0)" } ?? "Kotora counts from now on"
        return Text("\(start), and nothing is \(w.verb) yet. \(w.units.capitalized) you record show up here.")
            .font(Inter.regular(15)).foregroundStyle(p.dim)
    }

    // ---- lately ----

    private func lately(_ w: Words) -> some View {
        StatsCard(title: st.kind == .anime ? "Most watched lately" : "Most read lately") {
            if st.lately.isEmpty {
                Text(hasActivity ? "Nothing \(w.verb) in the last 30 days." : "Your most \(w.verb) titles of the last 30 days show here.")
                    .font(Inter.regular(15)).foregroundStyle(p.dim)
            } else {
                Caption("Last 30 days")
                ForEach(st.lately, id: \.mediaId) { t in TitleLine(t: t, title: titleOf(t), right: w.count(UInt64(t.units))) }
            }
        }
    }

    // ---- the list ----

    private var listCard: some View {
        let counts = Array(zip(ListStatus.allCases, st.statuses))
        let scored = st.scores.reduce(0, +)
        let mean = scored > 0 ? st.scores.enumerated().reduce(0.0) { $0 + Double($1.element) * Double($1.offset + 1) } / Double(scored) : nil
        return StatsCard(title: "Your list") {
            Caption("By status")
            GeometryReader { g in
                let total = max(1, counts.reduce(0) { $0 + Int($1.1) })
                let shown = counts.filter { $0.1 > 0 }
                HStack(spacing: 2) {
                    if st.titles == 0 { p.border }
                    ForEach(shown, id: \.0) { c in
                        p.status(c.0).frame(width: max(2, (g.size.width - CGFloat(shown.count - 1) * 2) * CGFloat(c.1) / CGFloat(total)))
                    }
                }
            }
            .frame(height: 10).clipShape(Capsule())
            LazyVGrid(columns: [GridItem(.flexible(), spacing: 16), GridItem(.flexible(), spacing: 16)], alignment: .leading, spacing: 4) {
                ForEach(counts, id: \.0) { c in
                    HStack(spacing: 8) {
                        Circle().fill(p.status(c.0)).frame(width: 8, height: 8)
                        Text(c.0.label(st.kind)).font(Inter.regular(13)).foregroundStyle(p.dim).lineLimit(1)
                        Spacer(minLength: 4)
                        Text(statsGrouped(n: UInt64(c.1))).font(Inter.medium(13)).monospacedDigit().foregroundStyle(p.text)
                    }
                    .padding(.vertical, 2)
                }
            }
            Caption("Scores").padding(.top, 6)
            Bars(values: st.scores, labels: (1...10).map(String.init), height: 64, gap: 6)
            Text(mean.map { "\(statsGrouped(n: UInt64(scored))) scored · mean \(String(format: "%.2f", $0)) (out of 10)" } ?? "No scores on your list yet.")
                .font(Inter.regular(13)).foregroundStyle(p.dim)
        }
    }

    // ---- genres and studios ----

    private var tags: some View {
        let anime = st.kind == .anime
        return StatsCard(title: anime ? "Genres and studios" : "Genres and authors") {
            Caption("Genres")
            if st.genres.isEmpty {
                Text("Genres come from the details pages Kotora has saved: open a title's page and it counts here.")
                    .font(Inter.regular(13)).foregroundStyle(p.dim)
            } else {
                Ranked(items: st.genres)
            }
            Caption(anime ? "Studios" : "Authors").padding(.top, 6)
            if st.makers.isEmpty {
                Text(anime ? "None saved yet." : "None on your list yet.").font(Inter.regular(13)).foregroundStyle(p.dim)
            } else {
                Ranked(items: st.makers)
            }
            if !st.genres.isEmpty || anime {
                Text("From the \(statsGrouped(n: UInt64(st.withDetails))) of your \(statsGrouped(n: UInt64(st.watched))) titles (not Plan to \(anime ? "watch" : "read")) whose details are saved: open a title's page to add it.")
                    .font(Inter.regular(12)).foregroundStyle(p.faint)
            }
        }
    }

    // ---- airing ----

    private var airing: some View {
        StatsCard(title: "Airing that you watch") {
            ForEach(st.airing, id: \.mediaId) { t in
                TitleLine(t: t, title: titleOf(t), right: t.total.map { "Ep \(t.progress) of \($0)" } ?? "Ep \(t.progress)")
            }
            Text("\(st.airing.count) airing · Airing shows when the next ones come out.").font(Inter.regular(12)).foregroundStyle(p.faint)
        }
    }
}

// ------------------------------------------------------------------------------- the calendar

/// The last weeks as columns of days (Monday on top), as many as fit; four steps of orange.
private struct CalendarCard: View {
    @Environment(\.palette) private var p
    let st: StatsData
    let w: Words

    static let gap: CGFloat = 3
    static let dayCol: CGFloat = 26

    /// The card's inner width, measured (the cells grow with it, then weeks drop off).
    @State private var width: CGFloat = 329

    private var layout: (cell: CGFloat, weeks: Int) {
        let room = width - Self.dayCol
        let fit = floor((room + Self.gap) / 53 - Self.gap)
        let cell = min(max(fit, 10), 15)
        return (cell, min(max(Int(floor((room + Self.gap) / (cell + Self.gap))), 8), 53))
    }

    private var first: Int64 {
        st.today - Int64(statsWeekday(day: st.today)) - Int64(layout.weeks - 1) * 7
    }

    private var inView: UInt64 {
        (0..<layout.weeks * 7).reduce(UInt64(0)) { acc, i in
            let day = first + Int64(i)
            return day > st.today ? acc : acc + UInt64(value(day))
        }
    }

    var body: some View {
        let (cell, weeks) = layout
        StatsCard(title: nil) {
            grid(cell: cell, weeks: weeks)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(GeometryReader { g in Color.clear.onAppear { width = g.size.width }.onChange(of: g.size.width) { _, w in width = w } })
            HStack(alignment: .top, spacing: 12) {
                Text(sinceNote).font(Inter.regular(12.5)).foregroundStyle(p.faint).frame(maxWidth: .infinity, alignment: .leading)
                legend
            }
        } head: {
            HStack {
                Text("Activity").font(Inter.semibold(16)).foregroundStyle(p.text)
                Spacer()
                Text("\(w.count(inView)) in \(weeks) weeks").font(Inter.medium(13)).monospacedDigit().foregroundStyle(p.dim)
            }
        }
    }

    private var sinceNote: String {
        if let d = st.sinceDay { return "Counted by Kotora since \(statsDateText(day: d)): what you record here, not what came before or from the website." }
        return "Counted by Kotora: what you record here, not what came before or from the website."
    }

    private func value(_ day: Int64) -> UInt32 {
        let i = day - st.firstDay
        return i >= 0 && i < Int64(st.days.count) ? st.days[Int(i)] : 0
    }

    private func shade(_ n: UInt32, _ max: UInt32) -> Color {
        if n == 0 { return p.border }
        let step = min(Swift.max((4 * n + Swift.max(max, 1) - 1) / Swift.max(max, 1), 1), 4)
        return p.accent.opacity([0.3, 0.5, 0.75, 1.0][Int(step) - 1])
    }

    @ViewBuilder private func grid(cell: CGFloat, weeks: Int) -> some View {
        let first = self.first
        let max = st.days.max() ?? 0
        VStack(alignment: .leading, spacing: 4) {
            // The months over the first week that ends in them.
            ZStack(alignment: .topLeading) {
                ForEach(0..<weeks, id: \.self) { wk in
                    let m = statsMonth(day: first + Int64(wk) * 7 + 6)
                    let prev = wk == 0 ? 0 : statsMonth(day: first + Int64(wk - 1) * 7 + 6)
                    if m != prev && (wk > 0 || weeks < 10) {
                        Text(Self.months[Int(m) - 1]).font(Inter.regular(10)).foregroundStyle(p.faint).fixedSize()
                            .offset(x: Self.dayCol + CGFloat(wk) * (cell + Self.gap))
                    }
                }
            }
            .frame(height: 14, alignment: .topLeading)
            HStack(alignment: .top, spacing: Self.gap) {
                VStack(alignment: .leading, spacing: Self.gap) {
                    ForEach(Array(["Mon", "", "Wed", "", "Fri", "", ""].enumerated()), id: \.offset) { d in
                        Text(d.element).font(Inter.regular(9.5)).foregroundStyle(p.faint).frame(height: cell)
                    }
                }
                .frame(width: Self.dayCol - Self.gap, alignment: .leading)
                ForEach(0..<weeks, id: \.self) { wk in
                    VStack(spacing: Self.gap) {
                        ForEach(0..<7, id: \.self) { d in
                            dayCell(first + Int64(wk * 7 + d), cell: cell, max: max)
                        }
                    }
                }
            }
            .accessibilityElement(children: .ignore)
            .accessibilityLabel("\(w.count(inView)) in \(weeks) weeks")
        }
    }

    @ViewBuilder private func dayCell(_ day: Int64, cell: CGFloat, max: UInt32) -> some View {
        let shape = RoundedRectangle(cornerRadius: 2.5)
        if day > st.today {
            Color.clear.frame(width: cell, height: cell)
        } else if let s = st.sinceDay, day < s, value(day) == 0 {
            // Before Kotora counted: not known, not "nothing".
            shape.stroke(p.border, lineWidth: 1).frame(width: cell, height: cell)
        } else {
            shape.fill(shade(value(day), max)).frame(width: cell, height: cell)
        }
    }

    private var legend: some View {
        HStack(spacing: 3) {
            Text("Less").font(Inter.regular(11)).foregroundStyle(p.faint).padding(.trailing, 4)
            ForEach(0..<5, id: \.self) { i in RoundedRectangle(cornerRadius: 2.5).fill(shade(UInt32(i), 4)).frame(width: 11, height: 11) }
            Text("More").font(Inter.regular(11)).foregroundStyle(p.faint).padding(.leading, 4)
        }
    }

    static let months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
}

// ------------------------------------------------------------------------------- the pieces

/// A card with a section title.
private struct StatsCard<Content: View, Head: View>: View {
    @Environment(\.palette) private var p
    let content: Content
    let head: Head

    init(title: String?, @ViewBuilder content: () -> Content) where Head == AnyView {
        self.content = content()
        self.head = AnyView(Group { if let title { StatsCardTitle(text: title) } })
    }

    init(title: String?, @ViewBuilder content: () -> Content, @ViewBuilder head: () -> Head) {
        self.content = content()
        self.head = head()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            head
            content
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(16)
        .background(RoundedRectangle(cornerRadius: 14).fill(p.surface))
        .overlay(RoundedRectangle(cornerRadius: 14).stroke(p.border, lineWidth: 1))
    }
}

private struct StatsCardTitle: View {
    @Environment(\.palette) private var p
    let text: String
    var body: some View { Text(text).font(Inter.semibold(16)).foregroundStyle(p.text) }
}

private struct Caption: View {
    @Environment(\.palette) private var p
    let text: String
    init(_ text: String) { self.text = text }
    var body: some View { Text(text.uppercased()).font(Inter.semibold(11)).tracking(0.4).foregroundStyle(p.faint) }
}

/// Small vertical bars with a label under each.
private struct Bars: View {
    @Environment(\.palette) private var p
    let values: [UInt32]
    let labels: [String]
    let height: CGFloat
    let gap: CGFloat

    var body: some View {
        let max = CGFloat(values.max() ?? 0).clamped(min: 1)
        HStack(alignment: .bottom, spacing: gap) {
            ForEach(values.indices, id: \.self) { i in
                let v = values[i]
                VStack(spacing: 4) {
                    Spacer(minLength: 0)
                    RoundedRectangle(cornerRadius: 2).fill(v == 0 ? p.border : p.accent)
                        .frame(height: v == 0 ? 2 : Swift.max(3, height * CGFloat(v) / max))
                    Text(i < labels.count ? labels[i] : "").font(Inter.regular(10)).monospacedDigit().foregroundStyle(p.faint)
                        .lineLimit(1).fixedSize()
                        .frame(height: 12)
                }
                .frame(maxWidth: .infinity)
            }
        }
        .frame(height: height + 16)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(zip(labels, values).filter { !$0.0.isEmpty }.map { "\($0.0): \($0.1)" }.joined(separator: ", "))
    }
}

/// Horizontal bars: a name, a bar, a count.
private struct Ranked: View {
    @Environment(\.palette) private var p
    let items: [NamedCount]

    var body: some View {
        let max = CGFloat(items.map(\.count).max() ?? 1).clamped(min: 1)
        VStack(spacing: 6) {
            ForEach(items, id: \.name) { it in
                HStack(spacing: 10) {
                    Text(it.name).font(Inter.regular(13)).foregroundStyle(p.text).lineLimit(1).frame(width: 120, alignment: .leading)
                    GeometryReader { g in
                        ZStack(alignment: .leading) {
                            Capsule().fill(p.border)
                            Capsule().fill(p.accent).frame(width: g.size.width * CGFloat(it.count) / max)
                        }
                    }
                    .frame(height: 8)
                    Text("\(it.count)").font(Inter.medium(13)).monospacedDigit().foregroundStyle(p.dim).frame(width: 28, alignment: .leading)
                }
            }
        }
    }
}

/// A title with a small poster and a number on the right.
private struct TitleLine: View {
    @Environment(\.palette) private var p
    let t: StatsTitle
    let title: String
    let right: String

    var body: some View {
        HStack(spacing: 10) {
            CoverTile(id: t.mediaId, initials: String(title.prefix(1)), url: t.coverUrl, width: 26, height: 38, radius: 4)
            Text(title).font(Inter.medium(15)).foregroundStyle(p.text).lineLimit(1)
            Spacer(minLength: 8)
            Text(right).font(Inter.medium(13)).monospacedDigit().foregroundStyle(p.dim)
        }
    }
}

private extension CGFloat {
    func clamped(min m: CGFloat) -> CGFloat { Swift.max(self, m) }
}
