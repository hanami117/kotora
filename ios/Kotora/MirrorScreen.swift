import SwiftUI

/// Mirror edits, as the desktop's Settings → Account row and its Queue part (mirror.rs):
/// - `MirrorSettingsRow`: "Mirror edits to AniList", its state line, the switch.
///   On reads the other list and opens `MirrorScreen` (the preview and the three choices);
///   nothing is saved until one is picked. Off drops what still waits for the mirror.
/// - `MirrorQueueSection`: the Sync queue's mirror part (website edits, conflicts, waiting,
///   failed, sent today, adds still looking up their id).
/// The mirror's queue goes with the app's flush and sync and the background task
/// (`CoreRepository.flush`, `background_refresh`): no scheduler of its own.
extension AppModel {
    var mirror: MirrorRepository? { repo as? MirrorRepository }
}

struct MirrorSettingsRow: View {
    @Environment(\.palette) private var p
    @EnvironmentObject private var model: AppModel
    @State private var state: MirrorState?
    @State private var busy: String?
    @State private var note: String?
    @State private var error: String?
    @State private var preview: MirrorPreview?
    @State private var loaded = false

    var body: some View {
        // A VStack, not a Group: `.task` on a Group with no child (state still nil) never runs,
        // so the row never loaded. The placeholder below keeps the row there while it loads.
        VStack(spacing: 0) {
            if state == nil {
                Rectangle().fill(p.border).frame(height: 1).padding(.leading, 16)
                VStack(alignment: .leading, spacing: 4) {
                    Text("Mirror edits").font(Inter.medium(15)).foregroundStyle(p.text)
                    Text(loaded ? "Couldn't read the mirror setting. Reopen Settings to try again." : "Loading…")
                        .font(Inter.regular(12.5)).foregroundStyle(loaded ? p.dropped : p.faint)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 16).padding(.vertical, 12)
            }
            if let s = state {
                VStack(alignment: .leading, spacing: 0) {
                    Rectangle().fill(p.border).frame(height: 1).padding(.leading, 16)
                    HStack(alignment: .top, spacing: 12) {
                        VStack(alignment: .leading, spacing: 4) {
                            Text(s.title).font(Inter.medium(15)).foregroundStyle(p.text)
                            Text(busy ?? s.note).font(Inter.regular(12.5)).foregroundStyle(p.faint)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                        Spacer(minLength: 8)
                        Toggle("", isOn: Binding(get: { s.on || preview != nil }, set: { set($0) }))
                            .labelsHidden().tint(p.accent)
                            .disabled(busy != nil || (!s.on && !s.canTurnOn) || model.isSample)
                            .accessibilityIdentifier("settings/mirror")
                    }
                    .padding(.horizontal, 16).padding(.vertical, 12)
                    if let note {
                        Text(note).font(Inter.regular(13)).foregroundStyle(p.text)
                            .padding(.horizontal, 16).padding(.bottom, 12).fixedSize(horizontal: false, vertical: true)
                    }
                    if let error {
                        Text(error).font(Inter.regular(13)).foregroundStyle(p.dropped)
                            .padding(.horizontal, 16).padding(.bottom, 12).fixedSize(horizontal: false, vertical: true)
                    }
                    if let a = s.alignManga, busy == nil, preview == nil {
                        Rectangle().fill(p.border).frame(height: 1).padding(.leading, 16)
                        HStack(alignment: .top, spacing: 12) {
                            VStack(alignment: .leading, spacing: 2) {
                                Text(a.title).font(Inter.medium(15)).foregroundStyle(p.text)
                                Text(a.note).font(Inter.regular(12.5)).foregroundStyle(p.faint).fixedSize(horizontal: false, vertical: true)
                            }
                            Spacer(minLength: 8)
                            MirrorButton(title: a.button) { await compare(s, mangaOnly: true) }
                        }
                        .padding(.horizontal, 16).padding(.vertical, 12)
                    }
                }
            }
        }
        .task(id: model.list.version) {
            state = await model.mirror?.mirrorState()
            loaded = true
        }
        .sheet(item: Binding(get: { preview.map(PreviewBox.init) }, set: { if $0 == nil { preview = nil } })) { box in
            NavigationStack {
                MirrorScreen(view: box.preview.view(), pick: { kind, mal, theirs in box.preview.pickStatus(kind: kind, malId: mal, mirrorStatus: theirs) },
                             pickAll: { kind, theirs in box.preview.pickAll(kind: kind, mirrorStatus: theirs) },
                             read: { box.preview.view() },
                             choose: { align in await choose(box.preview, align) },
                             cancel: { preview = nil; error = nil })
            }
            .environment(\.palette, p)
        }
    }

    private func set(_ on: Bool) {
        guard let s = state else { return }
        if on && !s.on && preview == nil { Task { await compare(s, mangaOnly: false) } }
        if !on && s.on { Task { await stop(s) } }
    }

    private func compare(_ s: MirrorState, mangaOnly: Bool) async {
        guard let m = model.mirror else { return }
        error = nil
        note = nil
        busy = mangaOnly ? s.readingManga : s.reading
        do {
            preview = try await m.mirrorCompare(mangaOnly: mangaOnly)
        } catch {
            self.error = message(of: error)
        }
        busy = nil
    }

    private func choose(_ pv: MirrorPreview, _ align: MirrorAlign) async {
        guard let m = model.mirror, let s = state else { return }
        preview = nil
        busy = s.saving
        do {
            note = try await m.mirrorChoose(pv, align)
            model.requestFlush()
        } catch {
            self.error = message(of: error)
        }
        busy = nil
        state = await m.mirrorState()
        await model.reload()
    }

    private func stop(_ s: MirrorState) async {
        guard let m = model.mirror else { return }
        error = nil
        busy = s.turningOff
        do {
            note = try await m.mirrorStop()
        } catch {
            self.error = message(of: error)
        }
        busy = nil
        state = await m.mirrorState()
        await model.reload()
    }
}

private struct PreviewBox: Identifiable {
    let preview: MirrorPreview
    var id: ObjectIdentifier { ObjectIdentifier(preview) }
}

/// The two lists compared: counts, the lists, the status conflicts to pick, what can't be
/// mirrored, what each choice queues, and the choices ({main} wins / Merge / Don't align / Cancel).
struct MirrorScreen: View {
    @Environment(\.palette) private var p
    @State var view: MirrorPreviewView
    let pick: (MediaKind, UInt32, Bool) -> Void
    let pickAll: (MediaKind, Bool) -> Void
    let read: () -> MirrorPreviewView
    let choose: (MirrorAlign) async -> Void
    let cancel: () -> Void

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                ForEach(Array(view.parts.enumerated()), id: \.offset) { _, part in partView(part) }
                ForEach(Array(view.explain.enumerated()), id: \.offset) { i, t in
                    Text(t).font(Inter.regular(13)).foregroundStyle(i == 3 ? p.faint : p.dim).fixedSize(horizontal: false, vertical: true)
                }
                FlowRow {
                    MirrorButton(title: view.mainWins) { await choose(.mainWins) }
                    MirrorButton(title: view.merge, primary: true) { await choose(.merge) }
                    MirrorButton(title: view.dontAlign) { await choose(.dontAlign) }
                    MirrorButton(title: view.cancel) { cancel() }
                }
            }
            .padding(16)
        }
        .background(p.bg)
        .navigationTitle("Mirror edits")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar { ToolbarItem(placement: .cancellationAction) { Button(view.cancel) { cancel() } } }
    }

    @ViewBuilder private func partView(_ part: MirrorPart) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            if let h = part.heading { Text(h).font(Inter.semibold(12)).tracking(0.6).foregroundStyle(p.faint) }
            Text(part.title).font(Inter.medium(15)).foregroundStyle(p.text).fixedSize(horizontal: false, vertical: true)
            FlowRow {
                ForEach(part.counts, id: \.key) { c in
                    let col = color(c.key)
                    Text("\(c.count) \(c.label)").font(Inter.medium(12)).foregroundStyle(col)
                        .padding(.horizontal, 8).frame(height: 22)
                        .background(Capsule().fill(col.opacity(0.12)))
                }
            }
            ForEach(part.groups, id: \.key) { g in
                DisclosureGroup {
                    VStack(alignment: .leading, spacing: 4) {
                        ForEach(Array(g.lines.enumerated()), id: \.offset) { _, l in
                            Text(l).font(Inter.regular(12.5)).foregroundStyle(p.dim).frame(maxWidth: .infinity, alignment: .leading)
                        }
                    }.padding(.top, 4)
                } label: {
                    Text("\(g.title) (\(g.lines.count))").font(Inter.medium(13)).foregroundStyle(p.text)
                }
                .tint(p.faint)
            }
            if let h = part.picksHeading {
                Text(h).font(Inter.semibold(11.5)).tracking(0.6).foregroundStyle(p.faint).padding(.top, 4)
                ForEach(part.picks, id: \.malId) { r in
                    VStack(alignment: .leading, spacing: 6) {
                        Text(r.title).font(Inter.regular(13)).foregroundStyle(p.text).lineLimit(1)
                        HStack(spacing: 8) {
                            chip(r.mainLabel, on: !r.mirrorPicked) { pick(part.kind, r.malId, false); view = read() }
                            chip(r.mirrorLabel, on: r.mirrorPicked) { pick(part.kind, r.malId, true); view = read() }
                        }
                    }
                }
                if let all = part.allLine {
                    Text(all).font(Inter.regular(13)).foregroundStyle(p.faint)
                    HStack(spacing: 8) {
                        chip(part.allMainLabel, on: part.allMainOn) { pickAll(part.kind, false); view = read() }
                        chip(part.allMirrorLabel, on: part.allMirrorOn) { pickAll(part.kind, true); view = read() }
                    }
                }
            }
        }
        .padding(14)
        .background(RoundedRectangle(cornerRadius: 12).fill(p.field))
        .overlay(RoundedRectangle(cornerRadius: 12).stroke(p.border, lineWidth: 1))
    }

    private func color(_ key: String) -> Color {
        switch key {
        case "only-main": p.watching
        case "only-mirror": p.plan
        case "different": p.onHold
        case "same": p.completed
        default: p.faint
        }
    }

    private func chip(_ text: String, on: Bool, _ action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Text(text).font(Inter.medium(12.5)).foregroundStyle(p.text).lineLimit(1)
                .padding(.horizontal, 10).frame(height: 30)
                .background(RoundedRectangle(cornerRadius: 8).fill(on ? p.accent.opacity(0.18) : p.raised))
                .overlay(RoundedRectangle(cornerRadius: 8).stroke(on ? p.accent : p.borderStrong, lineWidth: 1))
        }
        .buttonStyle(.borderless)
    }
}

/// The Sync queue page's mirror part, under the main queue (desktop `queue_section`).
struct MirrorQueueSection: View {
    @Environment(\.palette) private var p
    @EnvironmentObject private var model: AppModel
    @State private var q: MirrorQueue?

    var body: some View {
        VStack(spacing: 0) { // not a Group: its .task would never run while q is nil
            if let q {
                VStack(alignment: .leading, spacing: 0) {
                    HStack(spacing: 8) {
                        Text(q.heading).font(Inter.semibold(17)).foregroundStyle(p.text)
                        if let r = q.read { Text(r).font(Inter.regular(12)).foregroundStyle(p.faint) }
                    }
                    .padding(.horizontal, 20).padding(.top, 22)
                    if let t = q.trouble {
                        Text(t).font(Inter.regular(13)).foregroundStyle(p.text).fixedSize(horizontal: false, vertical: true)
                            .frame(maxWidth: .infinity, alignment: .leading).padding(12)
                            .background(RoundedRectangle(cornerRadius: 10).fill(p.onHold.opacity(0.1)))
                            .overlay(RoundedRectangle(cornerRadius: 10).stroke(p.onHold.opacity(0.35), lineWidth: 1))
                            .padding(.horizontal, 16).padding(.top, 10)
                    }
                    ForEach(q.edits, id: \.malId) { e in
                        card(title: e.title, why: e.why,
                             a: (e.mainHead, e.mainText, e.useMain.map { l in (l, { await act { try await $0.mirrorSettle(kind: e.kind, malId: e.malId, how: .useMain) } }) }),
                             b: (e.mirrorHead, e.mirrorText, e.useMirror.map { l in (l, { await act { try await $0.mirrorSettle(kind: e.kind, malId: e.malId, how: .useMirror) } }) }),
                             foot: (e.leave, { await act { try await $0.mirrorSettle(kind: e.kind, malId: e.malId, how: .leave) } }))
                    }
                    ForEach(q.conflicts, id: \.id) { c in
                        card(title: c.title, why: c.why,
                             a: (c.mineHead, c.mineText, (c.keep, { await act { try await $0.mirrorKeepMine(c.id) } })),
                             b: (c.theirsHead, c.theirsText, (c.take, { await act { try await $0.mirrorTakeTheirs(c.id) } })),
                             foot: nil)
                    }
                    ForEach(q.groups, id: \.key) { g in
                        SectionTitle(text: g.label)
                        Card {
                            ForEach(Array(g.rows.enumerated()), id: \.element.id) { i, r in opLine(r, divider: i < g.rows.count - 1) }
                        }
                    }
                    if !q.waits.isEmpty {
                        SectionTitle(text: q.waitsHeading)
                        Card {
                            ForEach(Array(q.waits.enumerated()), id: \.offset) { i, w in
                                ListLine(title: w.title, subtitle: w.why, divider: i < q.waits.count - 1) { EmptyView() } trailing: { EmptyView() }
                            }
                        }
                    }
                }
                .accessibilityIdentifier("queue/mirror")
            }
        }
        .task(id: model.list.version) { q = await model.mirror?.mirrorQueue() }
    }

    private func act(_ f: (MirrorRepository) async throws -> Void) async {
        guard let m = model.mirror else { return }
        do {
            try await f(m)
            await model.reload()
            model.requestFlush()
        } catch {
            model.snack = Snack(text: message(of: error))
        }
        q = await m.mirrorQueue()
    }

    private func opLine(_ r: MirrorOpRow, divider: Bool) -> some View {
        VStack(spacing: 0) {
            HStack(alignment: .top, spacing: 12) {
                VStack(alignment: .leading, spacing: 3) {
                    Text(r.title).font(Inter.medium(15)).foregroundStyle(p.text).lineLimit(1)
                    Text(r.what).font(Inter.regular(13)).monospacedDigit().foregroundStyle(p.dim)
                    if let n = r.note {
                        Text(n).font(Inter.regular(12)).foregroundStyle(r.noteIsError ? p.dropped : p.faint).lineLimit(2)
                    }
                    Text(r.age).font(Inter.regular(12)).foregroundStyle(p.faint)
                }
                Spacer(minLength: 4)
                if r.canDiscard {
                    MirrorButton(title: "Discard") { await act { try await $0.mirrorDiscard(r.id) } }
                } else if r.sent {
                    Label(r.state, systemImage: "checkmark").font(Inter.medium(12)).foregroundStyle(p.watching)
                } else {
                    Text(r.state).font(Inter.medium(11.5)).foregroundStyle(r.state == "Sending" ? p.dim : p.onHold)
                        .padding(.horizontal, 8).frame(height: 20)
                        .background(Capsule().fill((r.state == "Sending" ? p.dim : p.onHold).opacity(0.12)))
                }
            }
            .padding(.horizontal, 16).padding(.vertical, 10)
            .opacity(r.sent ? 0.6 : 1)
            if divider { Rectangle().fill(p.border).frame(height: 1).padding(.leading, 16) }
        }
    }

    typealias Act = (String, () async -> Void)

    private func card(title: String, why: String, a: (String, String, Act?), b: (String, String, Act?), foot: Act?) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 6) {
                Image(systemName: "exclamationmark.triangle.fill").font(.system(size: 13)).foregroundStyle(p.accent)
                Text(title).font(Inter.semibold(15)).foregroundStyle(p.text).lineLimit(2)
            }
            Text(why).font(Inter.regular(13)).foregroundStyle(p.dim).fixedSize(horizontal: false, vertical: true)
            side(a, on: true)
            side(b, on: false)
            if let foot { MirrorButton(title: foot.0) { await foot.1() } }
        }
        .padding(16)
        .background(RoundedRectangle(cornerRadius: 14).fill(p.surface))
        .overlay(RoundedRectangle(cornerRadius: 14).stroke(p.accent.opacity(0.45), lineWidth: 1))
        .padding(.horizontal, 16).padding(.top, 12)
    }

    private func side(_ s: (String, String, Act?), on: Bool) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(s.0).font(Inter.regular(12)).foregroundStyle(p.faint)
            Text(s.1).font(Inter.medium(15)).foregroundStyle(p.text).fixedSize(horizontal: false, vertical: true)
            if let act = s.2 { MirrorButton(title: act.0, primary: on) { await act.1() }.padding(.top, 2) }
        }
        .padding(12).frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 10).fill(p.field))
        .overlay(RoundedRectangle(cornerRadius: 10).stroke(on ? p.accent.opacity(0.5) : p.border, lineWidth: 1))
    }
}

struct MirrorButton: View {
    @Environment(\.palette) private var p
    let title: String
    var primary = false
    let action: () async -> Void
    @State private var busy = false

    var body: some View {
        Button {
            busy = true
            Task { await action(); busy = false }
        } label: {
            Text(title).font(Inter.medium(13)).foregroundStyle(primary ? p.onAccent : p.text)
                .padding(.horizontal, 14).frame(height: 34)
                .background(RoundedRectangle(cornerRadius: 8).fill(primary ? p.accent : p.raised))
                .overlay(RoundedRectangle(cornerRadius: 8).stroke(primary ? .clear : p.borderStrong, lineWidth: 1))
        }
        .buttonStyle(.borderless)
        .disabled(busy)
        .accessibilityIdentifier("mirror/\(title)")
    }
}

/// Wraps its children onto more lines when they don't fit (the desktop's `flex_wrap`).
struct FlowRow: Layout {
    var spacing: CGFloat = 8

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let w = proposal.width ?? .infinity
        var (x, y, line, width): (CGFloat, CGFloat, CGFloat, CGFloat) = (0, 0, 0, 0)
        for v in subviews {
            let s = v.sizeThatFits(.unspecified)
            if x > 0 && x + s.width > w { y += line + spacing; x = 0; line = 0 }
            x += s.width + spacing
            line = max(line, s.height)
            width = max(width, x - spacing)
        }
        return CGSize(width: proposal.width ?? width, height: y + line)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var (x, y, line): (CGFloat, CGFloat, CGFloat) = (bounds.minX, bounds.minY, 0)
        for v in subviews {
            let s = v.sizeThatFits(.unspecified)
            if x > bounds.minX && x + s.width > bounds.maxX { y += line + spacing; x = bounds.minX; line = 0 }
            v.place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(s))
            x += s.width + spacing
            line = max(line, s.height)
        }
    }
}
