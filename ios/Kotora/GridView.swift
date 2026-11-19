import SwiftUI

/// The score in the user's format (desktop `scores.rs`): the number, five small stars, or a face.
struct ScoreView: View {
    @Environment(\.palette) private var p
    let mark: ScoreMark?
    var size: CGFloat = 12.5
    /// Posters: a star before the number (desktop `poster_mark`).
    var star = true

    var body: some View {
        Group {
            if let m = mark {
                switch m.style {
                case .stars:
                    HStack(spacing: 1) {
                        ForEach(1...5, id: \.self) { i in
                            Image(systemName: "star.fill").font(.system(size: size - 3))
                                .foregroundStyle(i <= Int(m.stars) ? p.onHold : p.borderStrong)
                        }
                    }
                case .smiley:
                    let (icon, tint): (String, Color) = switch m.face ?? .neutral {
                    case .sad: ("face.dashed", p.dropped)
                    case .neutral: ("face.smiling.inverse", p.dim)
                    case .happy: ("face.smiling", p.watching)
                    }
                    Image(systemName: icon).font(.system(size: size + 2)).foregroundStyle(tint)
                case .number:
                    HStack(spacing: 3) {
                        if star { Image(systemName: "star.fill").font(.system(size: size - 3)).foregroundStyle(p.onHold) }
                        Text(m.text).font(Inter.medium(size)).monospacedDigit().foregroundStyle(p.dim)
                    }
                }
            } else {
                Text("★ –").font(Inter.regular(size)).foregroundStyle(p.faint)
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(mark.map { "Score \($0.words)" } ?? "No score")
    }
}

/// A pill on a poster: the page colour, nearly opaque, so it reads on any cover (desktop `badge`).
private struct Badge: View {
    @Environment(\.palette) private var p
    let text: String
    var dot: Color? = nil
    var filled = false

    var body: some View {
        HStack(spacing: 5) {
            if let dot { Circle().fill(dot).frame(width: 6, height: 6) }
            Text(text).font(Inter.semibold(10.5)).foregroundStyle(filled ? p.onAccent : p.text).lineLimit(1)
        }
        .padding(.horizontal, 7).frame(height: 20)
        .background(Capsule().fill(filled ? p.accent : p.bg.opacity(0.86)))
    }
}

/// The badges at a poster's top-left: rewatch or the run, a new episode, a change waiting.
private struct Badges: View {
    @Environment(\.palette) private var p
    let line: PageLine

    var body: some View {
        let r = line.row
        VStack(alignment: .leading, spacing: 4) {
            if r.rewatching {
                Badge(text: r.kind == .manga ? "Rereading" : "Rewatching", dot: p.completed)
            } else if r.airStatus == "currently_airing" && r.kind == .anime {
                Badge(text: "Airing", dot: p.air)
            } else if r.airStatus == "currently_publishing" || (r.kind == .manga && r.airStatus == "currently_airing") {
                Badge(text: "Publishing", dot: p.air)
            } else if line.onHiatus {
                Badge(text: "On hiatus", dot: p.dropped)
            }
            if let n = line.newEpisode { Badge(text: "Ep \(n) out", filled: true) }
            if r.waiting { Badge(text: "Waiting to sync", dot: p.onHold) }
        }
        .padding(6)
    }
}

/// Grid (dense posters, three across) and Large (cards, two across): desktop `grid.rs`/`mgrid.rs`.
struct PosterGrid: View {
    @Environment(\.palette) private var p
    let lines: [PageLine]
    let large: Bool
    let open: (Row) -> Void
    let plusOne: (Row) -> Void

    var body: some View {
        let cols = Array(repeating: GridItem(.flexible(), spacing: large ? 12 : 10, alignment: .top), count: large ? 2 : 3)
        LazyVGrid(columns: cols, spacing: large ? 14 : 12) {
            ForEach(lines, id: \.row.key) { line in
                card(line)
                    .contentShape(Rectangle())
                    .onTapGesture { open(line.row) }
                    .contextMenu { OpenOnMenuItems(kind: line.row.kind, id: line.row.id) }
                    .accessibilityElement(children: .combine)
                    .accessibilityAddTraits(.isButton)
                    .accessibilityAction(named: "Plus one") { if line.row.canPlusOne { plusOne(line.row) } }
            }
        }
        .padding(.horizontal, 16).padding(.top, 4)
    }

    @ViewBuilder private func card(_ line: PageLine) -> some View {
        let r = line.row
        let poster = Color.clear.aspectRatio(2 / 3, contentMode: .fit)
            .overlay(CoverTile(id: r.id, initials: r.initials, url: r.coverUrl, width: nil, height: 0, radius: large ? 0 : 8))
            .clipShape(RoundedRectangle(cornerRadius: large ? 0 : 8))
            .overlay(alignment: .topLeading) { Badges(line: line) }
            .overlay(alignment: .bottomTrailing) {
                if r.canPlusOne && !large {
                    Button { plusOne(r) } label: {
                        Text("+1").font(Inter.semibold(13)).foregroundStyle(p.text)
                            .frame(height: 28).padding(.horizontal, 11)
                            .background(Capsule().fill(p.bg.opacity(0.88)))
                            .overlay(Capsule().stroke(p.borderStrong, lineWidth: 1))
                    }
                    .buttonStyle(.borderless).padding(6)
                    .accessibilityLabel("Plus one \(r.kind.unit): \(r.title)")
                }
            }
        if large {
            VStack(alignment: .leading, spacing: 0) {
                poster
                VStack(alignment: .leading, spacing: 10) {
                    Text(r.title).font(Inter.semibold(14)).foregroundStyle(p.text).lineLimit(2)
                        .frame(height: 38, alignment: .topLeading)
                    bar(r).frame(height: 5)
                    HStack(spacing: 8) {
                        Text(epText(r)).font(Inter.medium(12)).monospacedDigit().foregroundStyle(p.dim)
                        if line.scoreMark != nil { ScoreView(mark: line.scoreMark, size: 12) }
                        Spacer(minLength: 0)
                        if r.canPlusOne {
                            Button { plusOne(r) } label: {
                                Text("+1").font(Inter.semibold(14)).foregroundStyle(p.text)
                                    .frame(width: 44, height: 30)
                                    .background(RoundedRectangle(cornerRadius: 8).fill(p.raised))
                                    .overlay(RoundedRectangle(cornerRadius: 8).stroke(p.borderStrong, lineWidth: 1))
                            }
                            .buttonStyle(.borderless)
                            .accessibilityLabel("Plus one \(r.kind.unit): \(r.title)")
                        }
                    }
                    .frame(height: 30)
                }
                .padding(12)
            }
            .background(RoundedRectangle(cornerRadius: 12).fill(p.surface))
            .clipShape(RoundedRectangle(cornerRadius: 12))
            .overlay(RoundedRectangle(cornerRadius: 12).stroke(p.border, lineWidth: 1))
        } else {
            VStack(alignment: .leading, spacing: 5) {
                poster
                Text(r.title).font(Inter.medium(12.5)).foregroundStyle(p.text).lineLimit(2)
                    .frame(height: 32, alignment: .topLeading)
                bar(r).frame(height: 3)
                HStack(spacing: 6) {
                    Text(epText(r)).font(Inter.regular(11.5)).monospacedDigit().foregroundStyle(p.dim)
                    Spacer(minLength: 0)
                    if line.scoreMark != nil { ScoreView(mark: line.scoreMark, size: 11.5) }
                }
            }
        }
    }

    private func epText(_ r: Row) -> String {
        "\(r.kind == .manga ? "Ch" : "Ep") \(r.progress) / \(r.total.map(String.init) ?? "?")"
    }

    private func bar(_ r: Row) -> some View {
        GeometryReader { g in
            ZStack(alignment: .leading) {
                Capsule().fill(p.border)
                Capsule().fill(p.status(r.rewatching ? .completed : r.status)).frame(width: g.size.width * fraction(r))
            }
        }
    }

    private func fraction(_ r: Row) -> CGFloat {
        guard let t = r.total, t > 0 else { return r.progress > 0 ? 0.15 : 0 }
        return min(1, CGFloat(r.progress) / CGFloat(t))
    }
}
