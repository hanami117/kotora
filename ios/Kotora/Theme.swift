import SwiftUI

/// Kotora's own colours: the desktop's tokens (`app/src/theme.rs`, design-spec §1), dark and light,
/// as in the mobile mockup. No system tint: Kotora looks like Kotora.
struct Palette {
    let bg, side, surface, raised, field, border, borderStrong, text, dim, faint: Color
    let accent, accentHi, onAccent, tint: Color
    let watching, completed, onHold, dropped, plan, air: Color
    let dark: Bool

    static let dark = Palette(
        bg: .hex(0x0f1626), side: .hex(0x121b2e), surface: .hex(0x182238), raised: .hex(0x1f2b45), field: .hex(0x141d31),
        border: .hex(0x26324d), borderStrong: .hex(0x34446a), text: .hex(0xedf0f7), dim: .hex(0xb7c0d2), faint: .hex(0x7d89a4),
        accent: .hex(0xf28a22), accentHi: .hex(0xff9d3f), onAccent: .hex(0x1d1208), tint: Color.hex(0xf28a22).opacity(0.14),
        watching: .hex(0x4cc38a), completed: .hex(0x6ea8ff), onHold: .hex(0xf2c94c), dropped: .hex(0xf4898f), plan: .hex(0xb29afb), air: .hex(0x4fd1e0),
        dark: true)

    static let light = Palette(
        bg: .hex(0xf6f7fb), side: .hex(0xeceff6), surface: .hex(0xffffff), raised: .hex(0xe4e8f1), field: .hex(0xffffff),
        border: .hex(0xdbe0ea), borderStrong: .hex(0xb9c1d3), text: .hex(0x131a2b), dim: .hex(0x454f68), faint: .hex(0x5f6982),
        accent: .hex(0xf28a22), accentHi: .hex(0xa64a00), onAccent: .hex(0x1d1208), tint: Color.hex(0xf28a22).opacity(0.16),
        watching: .hex(0x13704a), completed: .hex(0x2559c4), onHold: .hex(0x8a5d00), dropped: .hex(0xc02a35), plan: .hex(0x6b45d1), air: .hex(0x0a6d7a),
        dark: false)

    func status(_ s: ListStatus) -> Color {
        switch s {
        case .watching: return watching
        case .planToWatch: return plan
        case .completed: return completed
        case .onHold: return onHold
        case .dropped: return dropped
        }
    }
}

extension Color {
    static func hex(_ v: UInt32) -> Color {
        Color(.sRGB, red: Double((v >> 16) & 0xff) / 255, green: Double((v >> 8) & 0xff) / 255, blue: Double(v & 0xff) / 255)
    }

    /// CSS hsl() → SwiftUI (which takes HSB).
    static func hsl(_ h: Double, _ s: Double, _ l: Double) -> Color {
        let v = l + s * min(l, 1 - l)
        let sv = v == 0 ? 0 : 2 * (1 - l / v)
        return Color(hue: h / 360, saturation: sv, brightness: v)
    }
}

private struct PaletteKey: EnvironmentKey {
    static let defaultValue = Palette.dark
}

extension EnvironmentValues {
    var palette: Palette {
        get { self[PaletteKey.self] }
        set { self[PaletteKey.self] = newValue }
    }
}

/// Inter, bundled (the same files as the Android app, OFL).
enum Inter {
    static func regular(_ size: CGFloat) -> Font { .custom("Inter-Regular", size: size) }
    static func medium(_ size: CGFloat) -> Font { .custom("Inter-Medium", size: size) }
    static func semibold(_ size: CGFloat) -> Font { .custom("Inter-SemiBold", size: size) }
}

/// Picks the palette from the colour scheme in effect.
struct Themed<Content: View>: View {
    @Environment(\.colorScheme) private var scheme
    @ViewBuilder var content: Content

    var body: some View {
        content.environment(\.palette, scheme == .dark ? .dark : .light)
    }
}
