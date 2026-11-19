import SwiftUI
import UserNotifications

/// Settings' Appearance, Startup and Notifications sections, the Statistics link and "Fill volumes
/// from chapters…": the desktop's rows and words (`app/src/settings.rs`), saved in `config.toml`
/// under the desktop's keys (`Prefs`). The notifications themselves (asking, scheduling) are the
/// Airing page's: these switches only say which ones are wanted.
struct SettingsExtras: View {
    @Environment(\.palette) private var p
    @EnvironmentObject private var model: AppModel
    @ObservedObject private var prefs = Prefs.shared
    @State private var systemNotifications: UNAuthorizationStatus?
    @State private var fill = false
    @State private var titles = "romaji"
    @State private var keepHigher = true

    var body: some View {
        let s = prefs.settings
        VStack(alignment: .leading, spacing: 0) {
            SectionTitle(text: "Kotora")
            Card {
                NavigationLink {
                    StatsScreen()
                } label: {
                    ListLine(title: "Statistics", subtitle: "What you watched and read, by Kotora's count.", divider: false) {
                        Image(systemName: "chart.bar.xaxis").foregroundStyle(p.accent).frame(width: 22)
                    } trailing: { chevron }
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("statistics")
            }

            SectionTitle(text: "Appearance")
            Card {
                ListLine(title: "Theme", subtitle: "System follows your iPhone's light or dark setting.") { EmptyView() } trailing: {
                    picker(s.theme, [("system", "System"), ("light", "Light"), ("dark", "Dark")]) { prefs.set(.theme(value: $0)) }
                }
                if let src = model.repo as? ExtrasSource {
                    ListLine(title: "Titles", subtitle: "Romaji, English or Japanese titles everywhere.") { EmptyView() } trailing: {
                        picker(titles, [("romaji", "Romaji"), ("english", "English"), ("japanese", "Japanese")]) { v in
                            titles = v
                            Task {
                                try? await src.setTitleLanguage(v)
                                await model.reload()
                            }
                        }
                    }
                }
                ListLine(title: "Startup animation", subtitle: "Claw marks when Kotora opens.", divider: false) { EmptyView() } trailing: {
                    toggle(s.intro) { prefs.set(.intro(on: $0)) }
                }
            }

            if let src = model.repo as? ExtrasSource {
                SectionTitle(text: "Sync")
                Card {
                    ListLine(title: "Keep the higher progress in conflicts", subtitle: "When only the progress differs and yours is higher, Kotora sends yours.", divider: false) { EmptyView() } trailing: {
                        toggle(keepHigher) { on in
                            keepHigher = on
                            Task { do { try await src.setKeepHigherProgress(on) } catch { keepHigher = !on } }
                        }
                    }
                }
                .task {
                    titles = await src.titleLanguage()
                    keepHigher = await src.keepHigherProgress()
                }
            }

            SectionTitle(text: "Startup")
            Card {
                ListLine(title: "Open on", subtitle: "Home, or the page Kotora was on when it was last closed or hidden.") { EmptyView() } trailing: {
                    picker(s.openOn, [("home", "Home"), ("last", "Last page")]) { prefs.set(.openOn(value: $0)) }
                }
                ListLine(title: "Mode", subtitle: "Last used, Anime or Manga. Manga only when Manga is on.", divider: false) { EmptyView() } trailing: {
                    picker(s.startMode, [("last", "Last used"), ("anime", "Anime"), ("manga", "Manga")]) { prefs.set(.startMode(value: $0)) }
                }
            }

            SectionTitle(text: "Notifications")
            Card {
                ListLine(title: "New episode", subtitle: "When the next episode of a show you're watching is out.") { EmptyView() } trailing: {
                    toggle(s.notifyNewEpisode) { prefs.set(.notifyNewEpisode(on: $0)) }
                }
                ListLine(title: "Started airing", subtitle: "When a show on Plan to watch starts airing.", divider: model.mangaEnabled) { EmptyView() } trailing: {
                    toggle(s.notifyStartedAiring) { prefs.set(.notifyStartedAiring(on: $0)) }
                }
                if model.mangaEnabled {
                    ListLine(title: "New chapter", subtitle: "When a manga you're reading gets a new chapter on MangaUpdates.", divider: false) { EmptyView() } trailing: {
                        toggle(s.notifyNewChapter) { prefs.set(.notifyNewChapter(on: $0)) }
                    }
                }
            }
            if systemNotifications == .denied {
                HStack(alignment: .firstTextBaseline) {
                    Text("Notifications are off for Kotora in the iPhone's Settings.").font(Inter.regular(12)).foregroundStyle(p.faint)
                    Spacer()
                    Button("Open Settings") {
                        if let u = URL(string: UIApplication.openNotificationSettingsURLString) { UIApplication.shared.open(u) }
                    }
                    .font(Inter.medium(12)).foregroundStyle(p.accentHi)
                }
                .padding(.horizontal, 16).padding(.top, 6)
            }

            // Export / Import MyAnimeList XML (TransferScreen.swift).
            TransferSection()

            if model.mangaEnabled && !model.isSample {
                SectionTitle(text: "Manga")
                Card {
                    Button { fill = true } label: {
                        ListLine(title: "Fill volumes from chapters…", subtitle: "Volumes read from chapters read, with MangaDex's volume tables; never lowered.", divider: false) {
                            EmptyView()
                        } trailing: { chevron }
                    }
                    .buttonStyle(.plain)
                }
            }
        }
        .task { systemNotifications = await UNUserNotificationCenter.current().notificationSettings().authorizationStatus }
        .sheet(isPresented: $fill) { FillVolumesSheet() }
    }

    private var chevron: some View {
        Image(systemName: "chevron.right").font(.system(size: 13, weight: .semibold)).foregroundStyle(p.faint)
    }

    private func toggle(_ on: Bool, _ set: @escaping (Bool) -> Void) -> some View {
        Toggle("", isOn: Binding(get: { on }, set: set)).labelsHidden().tint(p.accent)
    }

    private func picker(_ value: String, _ items: [(String, String)], _ set: @escaping (String) -> Void) -> some View {
        Menu {
            ForEach(items, id: \.0) { it in
                Button { set(it.0) } label: {
                    if it.0 == value { Label(it.1, systemImage: "checkmark") } else { Text(it.1) }
                }
            }
        } label: {
            HStack(spacing: 4) {
                Text(items.first { $0.0 == value }?.1 ?? value).font(Inter.medium(14))
                Image(systemName: "chevron.up.chevron.down").font(.system(size: 11, weight: .semibold))
            }
            .foregroundStyle(p.accentHi)
            .padding(.horizontal, 10).frame(height: 34)
        }
    }
}

// --------------------------------------------------------------------------- fill volumes

/// "Fill volumes from chapters…" (the desktop's `volfill.rs`): looks every manga with chapters read
/// and volumes short of their total up on MangaDex, shows what would change, then queues it as
/// one change set; Undo takes back what hasn't been sent.
struct FillVolumesSheet: View {
    @Environment(\.palette) private var p
    @Environment(\.dismiss) private var dismiss
    @EnvironmentObject private var model: AppModel

    private enum Phase {
        case idle(candidates: UInt32)
        case busy(FillProgress)
        case preview(FillPreview)
        case done(queued: UInt32, undone: UInt32?)
    }

    @State private var job: VolumeFill?
    @State private var phase: Phase = .idle(candidates: 0)
    @State private var error: String?
    @State private var working = false

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    content
                    if let error {
                        Text(error).font(Inter.regular(13)).foregroundStyle(p.dropped)
                    }
                    Text(mangadexCredit() + ". Saved answers are used for 30 days; “Look up again” asks MangaDex anew.")
                        .font(Inter.regular(12)).foregroundStyle(p.faint)
                }
                .padding(16)
            }
            .background(p.bg)
            .navigationTitle("Fill volumes")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Close") { job?.cancel(); dismiss() }.foregroundStyle(p.accentHi)
                }
            }
        }
        .task { open() }
    }

    @ViewBuilder private var content: some View {
        switch phase {
        case .idle(let n):
            Text(n == 0 ? "No manga to look up: none has chapters read with volumes short of its total."
                 : "Sets volumes read from chapters read, with MangaDex’s volume tables. You see every change first; volumes are never lowered.")
                .font(Inter.regular(15)).foregroundStyle(p.dim)
            if n > 0 {
                button("Look up \(n) manga", primary: true) { lookUp(refresh: false) }
            }
        case .busy(let pr):
            HStack(spacing: 10) {
                ProgressView().tint(p.accent)
                Text("Looking up \(pr.of) manga on MangaDex… (\(pr.done) of \(pr.of))").font(Inter.regular(15)).foregroundStyle(p.dim)
            }
            ProgressView(value: Double(pr.done), total: Double(max(pr.of, 1))).tint(p.accent)
            button("Stop") { job?.cancel() }
        case .preview(let pv):
            preview(pv)
        case .done(let n, let undone):
            if let u = undone {
                Text("Took back \(plural(u, "change", "changes")) that hadn’t been sent yet.").font(Inter.regular(15)).foregroundStyle(p.text)
            } else {
                Text("\(plural(n, "volume change", "volume changes")) queued: they go to \(model.session.active?.service.label ?? "the service") at its pace.")
                    .font(Inter.regular(15)).foregroundStyle(p.text)
                button("Undo") { undo() }.disabled(working)
            }
        }
    }

    @ViewBuilder private func preview(_ pv: FillPreview) -> some View {
        FlowCounts(items: [
            (pv.fill.count, "volumes go up", p.watching, true),
            (pv.same.count, "already right", p.completed, false),
            (pv.higher.count, "higher than MangaDex (left alone)", p.onHold, false),
            (pv.notFound.count, "not found on MangaDex", p.faint, false),
        ])
        group("Volumes that go up", pv.fill.map { "\($0.title): Ch \($0.chapters) → Vol \($0.from) → \($0.to)" })
        group("Not found on MangaDex", pv.notFound)
        group("Higher than MangaDex (left alone)", pv.higher.map { "\($0.title): Vol \($0.from) (MangaDex: \($0.to))" })
        group("Already right", pv.same.map { "\($0.title): Vol \($0.from)" })
        if pv.fill.isEmpty {
            Text("Nothing to change.").font(Inter.medium(15)).foregroundStyle(p.text)
            button("Look up again") { lookUp(refresh: true) }
        } else {
            Text("Only volumes change, through the sync queue to \(model.session.active?.service.label ?? "the service") at its pace; Undo takes back what hasn’t been sent.")
                .font(Inter.regular(13)).foregroundStyle(p.dim)
            HStack(spacing: 8) {
                button("Fill \(plural(UInt32(pv.fill.count), "volume count", "volume counts"))", primary: true) { apply() }.disabled(working)
                button("Look up again") { lookUp(refresh: true) }
            }
        }
    }

    @ViewBuilder private func group(_ title: String, _ lines: [String]) -> some View {
        if !lines.isEmpty {
            VStack(alignment: .leading, spacing: 4) {
                Text(title.uppercased()).font(Inter.semibold(11)).tracking(0.4).foregroundStyle(p.faint)
                ForEach(lines, id: \.self) { Text($0).font(Inter.regular(13.5)).foregroundStyle(p.text) }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(12)
            .background(RoundedRectangle(cornerRadius: 10).fill(p.field))
            .overlay(RoundedRectangle(cornerRadius: 10).stroke(p.border, lineWidth: 1))
        }
    }

    private func button(_ title: String, primary: Bool = false, _ action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Text(title).font(Inter.medium(14)).foregroundStyle(primary ? p.onAccent : p.text)
                .padding(.horizontal, 14).frame(height: 36)
                .background(RoundedRectangle(cornerRadius: 9).fill(primary ? p.accent : p.raised))
                .overlay(RoundedRectangle(cornerRadius: 9).stroke(primary ? .clear : p.borderStrong, lineWidth: 1))
        }
        .buttonStyle(.borderless)
    }

    private func plural(_ n: UInt32, _ one: String, _ many: String) -> String { n == 1 ? "1 \(one)" : "\(n) \(many)" }

    // ---- the work: blocking calls on a worker ----

    private func open() {
        guard job == nil, let src = model.repo as? InsightsSource else { return }
        do {
            job = try src.volumeFill()
            let j = job
            Task {
                let n = (try? await Task.detached { try j?.candidates() }.value) ?? 0
                phase = .idle(candidates: n ?? 0)
            }
        } catch {
            self.error = message(of: error)
        }
    }

    private func lookUp(refresh: Bool) {
        guard let j = job else { return }
        error = nil
        phase = .busy(FillProgress(done: 0, of: 0))
        let poll = Task {
            while !Task.isCancelled {
                phase = .busy(j.progress())
                try? await Task.sleep(for: .milliseconds(150))
            }
        }
        Task {
            let r = await Task.detached { Result { try j.lookUp(refresh: refresh) } }.value
            poll.cancel()
            switch r {
            case .success(let pv) where pv.stopped: phase = .idle(candidates: (try? j.candidates()) ?? 0)
            case .success(let pv): phase = .preview(pv)
            case .failure(let e):
                error = message(of: e)
                phase = .idle(candidates: (try? j.candidates()) ?? 0)
            }
        }
    }

    private func apply() {
        guard let j = job else { return }
        working = true
        Task {
            let r = await Task.detached { Result { try j.apply() } }.value
            working = false
            switch r {
            case .success(let n):
                phase = .done(queued: n, undone: nil)
                model.sync()
            case .failure(let e): error = message(of: e)
            }
        }
    }

    private func undo() {
        guard let j = job else { return }
        working = true
        Task {
            let r = await Task.detached { Result { try j.undo() } }.value
            working = false
            switch r {
            case .success(let n):
                phase = .done(queued: 0, undone: n)
                await model.reload()
            case .failure(let e): error = message(of: e)
            }
        }
    }
}

/// The preview's counts as pills ("go up" always, the others when they count something).
private struct FlowCounts: View {
    @Environment(\.palette) private var p
    let items: [(Int, String, Color, Bool)]

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            ForEach(items.filter { $0.0 > 0 || $0.3 }, id: \.1) { it in
                HStack(spacing: 6) {
                    Text("\(it.0)").font(Inter.semibold(13)).monospacedDigit()
                    Text(it.1).font(Inter.regular(13))
                }
                .foregroundStyle(it.2)
                .padding(.horizontal, 10).frame(height: 26)
                .background(Capsule().fill(it.2.opacity(0.14)))
            }
        }
    }
}
