import SwiftUI

/// The startup intro, the desktop's (`app/src/intro.rs`, the spec: geometry, timeline, easings and
/// colours are taken from there one to one): the app opens under a tiger-orange layer with 小虎 in
/// the middle; three claw slashes rip across it 90 ms apart, the layer splits along them into four
/// strips that slide away and fade, and the app is underneath. 0.8 s. A tap skips it.
///
/// - **The clock** starts at the first frame drawn with the layer (`onAppear`), then the solid
///   orange holds `lead` (0.10 s) before the motion: launch work that hitches a frame costs no motion.
/// - **When:** a cold start only (the process starting), never coming back from the background.
///   Settings → Appearance → Startup animation turns it off (`[display] intro`).
/// - **Reduce Motion:** the orange layer (with 小虎) holds 0.12 s, then fades over 0.25 s (ease-out).
/// - Vectors in one 1600×1000 box that covers the screen (one scale, centred), drawn by a `Canvas`
///   per frame from the time: the same tracks the desktop's scenes evaluate (`Intro::frozen`).
struct IntroView: View {
    @Environment(\.palette) private var p
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    /// Called once it is over (or skipped): the caller drops the layer.
    let done: () -> Void

    @State private var start: Date?
    @State private var skipped = false

    var body: some View {
        let reduced = reduceMotion
        TimelineView(.animation(paused: start == nil)) { tl in
            let t = Self.frozen ?? start.map { Float(tl.date.timeIntervalSince($0)) } ?? 0
            Canvas(rendersAsynchronously: false) { ctx, size in
                IntroDrawing(t: max(0, t), reduced: reduced, bg: p.bg, light: !p.dark).draw(&ctx, size)
            }
        }
        .ignoresSafeArea()
        .opacity(skipped ? 0 : 1)
        .contentShape(Rectangle())
        .onTapGesture { skip() }
        .accessibilityElement()
        .accessibilityLabel("Kotora")
        .accessibilityAddTraits(.isImage)
        .accessibilityAction { skip() }
        .task {
            if Self.frozen != nil { return }
            // The first frame is up with the layer opaque: hold `lead`, then run.
            try? await Task.sleep(for: .seconds(IntroTimeline.lead))
            guard !skipped else { return }
            start = Date()
            let total = reduced ? IntroTimeline.reducedTotal : IntroTimeline.total
            try? await Task.sleep(for: .seconds(Double(total)))
            if !skipped { done() }
        }
    }

    /// Debug builds: `-introAt 0.3` holds the intro at that time of its run (screenshots).
    static var frozen: Float? {
        #if DEBUG
        if let v = UserDefaults.standard.object(forKey: "introAt") as? NSNumber { return v.floatValue }
        if let s = UserDefaults.standard.string(forKey: "introAt") { return Float(s) }
        #endif
        return nil
    }

    /// A tap: the layer goes at once (its own 0.15 s fade, as on the desktop).
    private func skip() {
        guard !skipped else { return }
        withAnimation(.easeOut(duration: 0.15)) { skipped = true }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.16) { done() }
    }
}

// ------------------------------------------------------------------------------- the timeline

/// A CSS cubic-bezier easing (Rune's `Easing::cubic`).
struct Ease {
    let x1, y1, x2, y2: Float
    static let linear = Ease(x1: 0, y1: 0, x2: 1, y2: 1)
    static let easeIn = Ease(x1: 0.42, y1: 0, x2: 1, y2: 1)
    static let easeOut = Ease(x1: 0, y1: 0, x2: 0.58, y2: 1)

    func eval(_ x: Float) -> Float {
        if x <= 0 { return 0 }
        if x >= 1 { return 1 }
        func bez(_ t: Float, _ a: Float, _ b: Float) -> Float {
            let u = 1 - t
            return 3 * u * u * t * a + 3 * u * t * t * b + t * t * t
        }
        // Solve x(t) = x by bisection (monotonic for 0 ≤ x1, x2 ≤ 1).
        var lo: Float = 0, hi: Float = 1, t = x
        for _ in 0..<24 {
            t = (lo + hi) / 2
            if bez(t, x1, x2) < x { lo = t } else { hi = t }
        }
        return bez(t, y1, y2)
    }
}

/// One property's change: it starts `at` seconds into the run and takes `dur`.
struct Track {
    let at, dur: Float
    let ease: Ease

    func at(_ t: Float) -> Float {
        if dur <= 0 { return t >= at ? 1 : 0 }
        return ease.eval(min(max((t - at) / dur, 0), 1))
    }
}

/// The desktop's timeline (`intro.rs`, "the timeline"), in seconds from the start of the run.
enum IntroTimeline {
    static let hold: Float = 0.06
    static let slashGap: Float = 0.09
    static let slash: Float = 0.12
    static let splitAt: Float = 0.33
    static let split: Float = 0.45
    /// The solid orange before the run.
    static let lead: Double = 0.10
    static let total: Float = 0.8
    static let reducedTotal: Float = 0.4

    static let reducedFade = Track(at: 0.12, dur: 0.25, ease: .easeOut)
    static func slashTrack(_ k: Int) -> Track { Track(at: hold + slashGap * Float(k), dur: slash, ease: Ease(x1: 0.3, y1: 0, x2: 0.2, y2: 1)) }
    static let cutFade = Track(at: splitAt + 0.04, dur: 0.16, ease: .easeIn)
    static let stripMove = Track(at: splitAt, dur: split, ease: Ease(x1: 0.35, y1: 0, x2: 0.55, y2: 1))
    static let stripFade = Track(at: splitAt + 0.22, dur: split - 0.22, ease: .easeIn)
    static let baseOff = Track(at: splitAt, dur: 0, ease: .linear)
    static let nameFade = Track(at: splitAt - 0.04, dur: 0.14, ease: .easeIn)
}

// ------------------------------------------------------------------------------- the geometry

/// The shapes in the desktop's 1600×1000 box (`intro.rs`, "the geometry").
enum IntroGeometry {
    static let vw: Float = 1600, vh: Float = 1000
    static let samples = 28
    static let reach: Float = 1400

    typealias P = SIMD2<Float>

    static func claw(_ k: Int) -> [P] {
        let dx = (Float(k) - 1) * 210
        let bow: Float = [52, 70, 58][k]
        let p0 = P(1250 + dx, -70), p2 = P(350 + dx, 1070)
        let n = normal(p0, p2)
        let mid = (p0 + p2) / 2 + n * bow
        return [p0, mid, p2]
    }

    static func normal(_ a: P, _ b: P) -> P {
        let d = b - a
        let len = max((d.x * d.x + d.y * d.y).squareRoot(), 1e-3)
        return P(-d.y / len, d.x / len)
    }

    static func bezier(_ c: [P], _ t: Float) -> P {
        let u = 1 - t
        return u * u * c[0] + 2 * u * t * c[1] + t * t * c[2]
    }

    static func bezierNormal(_ c: [P], _ t: Float) -> P {
        let d = 2 * (1 - t) * (c[1] - c[0]) + 2 * t * (c[2] - c[1])
        return normal(.zero, d)
    }

    /// A claw mark's outline around line `k`: sharp where it starts, widest a third of the way in.
    static func mark(_ k: Int, width: Float, shift: Float) -> [P] {
        let c = claw(k)
        let a: Float = 0.55, b: Float = 1.15
        let peak = pow(a / (a + b), a) * pow(b / (a + b), b)
        var left: [P] = [], right: [P] = []
        for i in 0...samples {
            let t = Float(i) / Float(samples)
            let h = width / 2 * pow(t, a) * pow(1 - t, b) / peak
            let p = bezier(c, t), n = bezierNormal(c, t)
            let o = shift * width
            left.append(p + n * (o + h))
            right.append(p + n * (o - h))
        }
        return left + right.reversed()
    }

    /// Claw line `k`, carried on straight past both ends.
    static func edge(_ k: Int) -> [P] {
        let c = claw(k)
        func unit(_ a: P, _ b: P) -> P {
            let d = b - a
            let len = max((d.x * d.x + d.y * d.y).squareRoot(), 1e-3)
            return d / len
        }
        let d0 = unit(c[1], c[0]), d2 = unit(c[1], c[2])
        var pts = [c[0] + d0 * reach]
        pts += (0...samples).map { bezier(c, Float($0) / Float(samples)) }
        pts.append(c[2] + d2 * reach)
        return pts
    }

    /// Orange strip `k` (0 to 3, left to right).
    static func strip(_ k: Int) -> [P] {
        switch k {
        case 0:
            let e = edge(0)
            return e + [P(-reach, e.last!.y), P(-reach, e[0].y)]
        case 3:
            let e = edge(2)
            return e + [P(vw + reach, e.last!.y), P(vw + reach, e[0].y)]
        default:
            return edge(k - 1) + edge(k).reversed()
        }
    }

    /// Where strip `k` goes: across the claws away from the middle, and along them.
    static func stripAway(_ k: Int) -> P {
        let c = claw(1)
        let n = normal(c[0], c[2])
        let d = P(n.y, -n.x)
        let across: Float = [360, 140, -140, -360][k]
        let along: Float = [700, -560, 600, -680][k]
        return n * across + d * along
    }

    // Built once: the shapes never change, only how they are placed.
    static let strips = (0..<4).map { strip($0) }
    static let aways = (0..<4).map { stripAway($0) }
    static let claws = (0..<3).map { claw($0) }
    /// Per claw: rim, cut, glint.
    static let marks: [[[P]]] = (0..<3).map { k in
        let w: Float = [30, 38, 32][k]
        return [mark(k, width: w * 1.5, shift: 0), mark(k, width: w, shift: 0), mark(k, width: w * 0.26, shift: 0.24)]
    }
}

/// How the box lies on the screen: one scale that covers it, centred.
struct IntroFit {
    let w, h, k: CGFloat

    init(_ size: CGSize) {
        w = max(size.width, 1)
        h = max(size.height, 1)
        k = max(w / CGFloat(IntroGeometry.vw), h / CGFloat(IntroGeometry.vh))
    }

    func pt(_ p: IntroGeometry.P) -> CGPoint {
        CGPoint(x: (CGFloat(p.x) - CGFloat(IntroGeometry.vw) / 2) * k + w / 2,
                y: (CGFloat(p.y) - CGFloat(IntroGeometry.vh) / 2) * k + h / 2)
    }

    func path(_ pts: [IntroGeometry.P]) -> Path {
        var path = Path()
        path.addLines(pts.map(pt))
        path.closeSubpath()
        return path
    }
}

/// One frame of the intro at time `t` of the run.
struct IntroDrawing {
    let t: Float
    let reduced: Bool
    let bg: Color
    let light: Bool

    static let accent = Color.hex(0xf28a22)
    static let rim = Color.hex(0xffb866)
    static let glint = Color.hex(0xfff6e6).opacity(0.95)

    /// 小虎 over the orange: the light theme's on-accent brown, cream in the dark one.
    var nameColor: Color { light ? .hex(0x1d1208) : .hex(0xfff7ec) }

    func draw(_ ctx: inout GraphicsContext, _ size: CGSize) {
        typealias T = IntroTimeline
        let full = Path(CGRect(origin: .zero, size: size))
        // 132 points per glyph on the desktop; a little less on a narrow phone.
        let glyph = min(132, size.width * 0.3)
        let name = Kanji.name(size: glyph, center: CGPoint(x: size.width / 2, y: size.height / 2))

        if reduced {
            ctx.opacity = Double(1 - T.reducedFade.at(t))
            ctx.fill(full, with: .color(Self.accent))
            ctx.fill(name, with: .color(nameColor))
            return
        }

        let fit = IntroFit(size)
        // One solid layer under the strips until they part (no seams).
        if T.baseOff.at(t) < 1 {
            ctx.fill(full, with: .color(Self.accent))
        }
        let m = CGFloat(T.stripMove.at(t))
        let stripAlpha = Double(1 - T.stripFade.at(t))
        for k in 0..<4 {
            var c = ctx
            let a = IntroGeometry.aways[k]
            c.translateBy(x: CGFloat(a.x) * fit.k * m, y: CGFloat(a.y) * fit.k * m)
            c.opacity = stripAlpha
            c.fill(fit.path(IntroGeometry.strips[k]), with: .color(Self.accent))
        }
        do {
            var c = ctx
            c.opacity = Double(1 - T.nameFade.at(t))
            c.fill(name, with: .color(nameColor))
        }
        let cutAlpha = Double(1 - T.cutFade.at(t))
        for k in 0..<3 {
            // The mark grows out of its starting point: it rips across from the top right.
            let s = CGFloat(max(T.slashTrack(k).at(t), 0.001))
            let o = fit.pt(IntroGeometry.claws[k][0])
            var c = ctx
            c.opacity = cutAlpha
            c.translateBy(x: o.x, y: o.y)
            c.scaleBy(x: s, y: s)
            c.translateBy(x: -o.x, y: -o.y)
            let parts = IntroGeometry.marks[k]
            c.fill(fit.path(parts[0]), with: .color(Self.rim))
            c.fill(fit.path(parts[1]), with: .color(bg))
            c.fill(fit.path(parts[2]), with: .color(Self.glint))
        }
    }
}

#if DEBUG
#Preview("Intro, mid-run") {
    Canvas { ctx, size in IntroDrawing(t: 0.3, reduced: false, bg: .hex(0x0f1626), light: false).draw(&ctx, size) }
        .ignoresSafeArea()
}
#endif
