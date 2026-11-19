import SwiftUI

/// The mockup's cover tile: a 160° gradient from two colours picked from the id, with initials
/// (design-spec §3). No covers are fetched in the spike.
struct CoverTile: View {
    let id: UInt32
    let initials: String
    /// `nil`: as wide as the space given (the details page's blurred hero).
    var width: CGFloat? = 52
    var height: CGFloat = 74
    var radius: CGFloat = 8

    var body: some View {
        let h1 = Double((id * 47) % 360)
        let h2 = (h1 + 40 + Double((id * 13) % 50)).truncatingRemainder(dividingBy: 360)
        RoundedRectangle(cornerRadius: radius)
            .fill(LinearGradient(colors: [.hsl(h1, 0.55, 0.48), .hsl(h2, 0.50, 0.30)],
                                 startPoint: UnitPoint(x: 0.33, y: 0), endPoint: UnitPoint(x: 0.67, y: 1)))
            .overlay(Text(initials).font(Inter.semibold((width ?? 0) > 80 ? 26 : 15)).foregroundStyle(.white.opacity(0.92)))
            .frame(width: width, height: height)
            .frame(maxWidth: width == nil ? .infinity : nil)
    }
}

/// The top bar: tiger, title, trailing items.
struct AppBar<Trailing: View>: View {
    @Environment(\.palette) private var p
    let title: String
    @ViewBuilder var trailing: Trailing

    var body: some View {
        HStack(spacing: 12) {
            Image("Logo").resizable().frame(width: 30, height: 30)
            Text(title).font(Inter.semibold(21)).foregroundStyle(p.text)
            Spacer()
            trailing
        }
        .padding(.leading, 16).padding(.trailing, 12).frame(height: 56)
    }
}

struct Avatar: View {
    var letter = "S"
    var anilist = false
    var size: CGFloat = 32

    var body: some View {
        Circle()
            .fill(LinearGradient(colors: anilist ? [.hex(0x2b9fd8), .hex(0x1d6fa5)] : [.hex(0x3b5ba8), .hex(0x2d4c8f)],
                                 startPoint: .top, endPoint: .bottom))
            .overlay(Text(letter).font(Inter.semibold(size * 0.4)).foregroundStyle(.white))
            .frame(width: size, height: size)
    }
}

enum TagStyle { case new, air, wait, plain }

struct Tag: View {
    @Environment(\.palette) private var p
    let text: String
    var style: TagStyle = .plain

    var body: some View {
        let c: Color = switch style {
        case .new: p.accentHi
        case .air: p.air
        case .wait: p.onHold
        case .plain: p.dim
        }
        Text(text).font(Inter.medium(11.5)).foregroundStyle(c)
            .padding(.horizontal, 7).frame(height: 20)
            .background(RoundedRectangle(cornerRadius: 6).fill(style == .plain ? p.raised : c.opacity(0.14)))
    }
}

/// Anime | Manga, This week | Season, System | Light | Dark.
struct Segmented<T: Hashable>: View {
    @Environment(\.palette) private var p
    let items: [(T, String)]
    @Binding var selection: T
    var height: CGFloat = 34

    var body: some View {
        HStack(spacing: 0) {
            ForEach(items, id: \.0) { item in
                let on = item.0 == selection
                Text(item.1).font(Inter.medium(height < 34 ? 13 : 14))
                    .foregroundStyle(on ? p.text : p.dim)
                    .frame(maxWidth: .infinity).frame(height: height)
                    .background(RoundedRectangle(cornerRadius: 8).fill(on ? (p.dark ? p.raised : .white) : .clear)
                        .shadow(color: .black.opacity(on ? (p.dark ? 0.25 : 0.12) : 0), radius: 2, y: 1))
                    .contentShape(Rectangle())
                    .onTapGesture { withAnimation(.easeOut(duration: 0.15)) { selection = item.0 } }
            }
        }
        .padding(3)
        .background(RoundedRectangle(cornerRadius: 10).fill(p.field))
        .overlay(RoundedRectangle(cornerRadius: 10).stroke(p.borderStrong, lineWidth: 1))
    }
}

struct Card<Content: View>: View {
    @Environment(\.palette) private var p
    @ViewBuilder var content: Content

    var body: some View {
        VStack(spacing: 0) { content }
            .background(RoundedRectangle(cornerRadius: 14).fill(p.surface))
            .overlay(RoundedRectangle(cornerRadius: 14).stroke(p.border, lineWidth: 1))
            .padding(.horizontal, 16)
    }
}

struct SectionTitle: View {
    @Environment(\.palette) private var p
    let text: String
    var trailing: String? = nil

    var body: some View {
        HStack {
            Text(text.uppercased()).font(Inter.semibold(12)).tracking(0.6).foregroundStyle(p.faint)
            Spacer()
            if let trailing { Text(trailing).font(Inter.regular(12)).foregroundStyle(p.faint) }
        }
        .padding(.horizontal, 20).padding(.top, 18).padding(.bottom, 8)
    }
}

/// One line in a settings card.
struct ListLine<Leading: View, Trailing: View>: View {
    @Environment(\.palette) private var p
    let title: String
    var subtitle: String? = nil
    var divider = true
    @ViewBuilder var leading: Leading
    @ViewBuilder var trailing: Trailing

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 12) {
                leading
                VStack(alignment: .leading, spacing: 2) {
                    Text(title).font(Inter.medium(15)).foregroundStyle(p.text)
                    if let subtitle { Text(subtitle).font(Inter.regular(12.5)).foregroundStyle(p.faint) }
                }
                Spacer(minLength: 8)
                trailing
            }
            .padding(.horizontal, 16).padding(.vertical, 12)
            if divider { Rectangle().fill(p.border).frame(height: 1).padding(.leading, 16) }
        }
    }
}

struct ProgressLine: View {
    @Environment(\.palette) private var p
    let row: Row

    var body: some View {
        HStack(spacing: 8) {
            GeometryReader { g in
                ZStack(alignment: .leading) {
                    Capsule().fill(p.raised)
                    Capsule().fill(p.status(row.rewatching ? .watching : row.status))
                        .frame(width: g.size.width * fraction)
                }
            }
            .frame(width: 64, height: 4)
            Text("\(row.progress) / \(row.total.map(String.init) ?? "?")").font(Inter.medium(12.5)).monospacedDigit().foregroundStyle(p.dim)
            Text("★ \(row.score ?? "–")").font(Inter.regular(12.5)).foregroundStyle(p.faint)
        }
    }

    private var fraction: CGFloat {
        guard let t = row.total, t > 0 else { return row.progress > 0 ? 0.15 : 0 }
        return min(1, CGFloat(row.progress) / CGFloat(t))
    }
}

/// "Episode 6 saved · Undo" at the bottom of the list.
struct SnackBar: View {
    @EnvironmentObject private var model: CoreModel
    let snack: CoreModel.Snack

    var body: some View {
        HStack(spacing: 12) {
            Text(snack.text).font(Inter.regular(14)).foregroundStyle(Color.hex(0xedf0f7)).lineLimit(2)
            Spacer(minLength: 4)
            if let edit = snack.edit {
                Button("Undo") { model.undo(edit) }
                    .font(Inter.semibold(14)).foregroundStyle(Color.hex(0xff9d3f))
            }
        }
        .padding(.horizontal, 16).frame(minHeight: 48)
        .background(RoundedRectangle(cornerRadius: 12).fill(Color.hex(0x2a3550)))
        .shadow(color: .black.opacity(0.3), radius: 12, y: 4)
        .padding(.horizontal, 12).padding(.bottom, 8)
        .task(id: snack.id) {
            try? await Task.sleep(for: .seconds(5))
            if model.snack?.id == snack.id { withAnimation { model.snack = nil } }
        }
    }
}
