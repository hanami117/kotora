import SwiftUI

/// The sync queue, as the desktop's: conflicts as cards (Keep mine / Take theirs), then Waiting,
/// Failed and Sent today. Every signed-in account's changes.
struct QueueScreen: View {
    @Environment(\.palette) private var p
    @EnvironmentObject private var model: AppModel
    @State private var items: [QueueItem] = []
    @State private var loaded = false

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                SignInAgainBanner(queue: true).padding(.top, 6)
                notices
                if loaded && items.isEmpty {
                    VStack(spacing: 4) {
                        Text("Nothing waiting").font(Inter.semibold(17)).foregroundStyle(p.text)
                        Text("Your changes go here first, then to \(model.session.active?.service.label ?? "the service").")
                            .font(Inter.regular(13)).foregroundStyle(p.dim).multilineTextAlignment(.center)
                    }
                    .frame(maxWidth: .infinity).padding(.top, 80).padding(.horizontal, 24)
                }
                ForEach(items.filter { $0.state == .conflict }, id: \.id) { ConflictCard(item: $0).padding(.top, 12) }
                group("Waiting", items.filter { $0.state == .waiting || $0.state == .sending }, count: true)
                group("Failed", items.filter { $0.state == .failed }, count: true)
                group("Sent today", items.filter { $0.state == .sent }, count: false)
            }
            .padding(.bottom, 32)
        }
        .background(p.bg)
        .navigationTitle("Sync queue")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button("Sync now") { model.syncNow() }.disabled(model.list.syncing || model.isSample)
            }
        }
        .refreshable { await model.refresh() }
        .task(id: model.list.version) {
            items = await model.editing?.queueItems() ?? []
            loaded = true
        }
    }

    /// The desktop's notices: an expired sign-in, offline with changes waiting, another problem.
    @ViewBuilder private var notices: some View {
        let l = model.list
        let service = model.session.active?.service.label ?? "the service"
        let n = l.waiting
        if !l.signInAgain && l.offline && n > 0 {
            QueueNotice(text: "You are offline. Your changes are kept here and go out when \(service) answers again.", color: p.onHold) {
                QueueButton(title: "Try now") { model.syncNow() }
            }
        }
        if let problem = l.problem, !l.offline, !l.signInAgain {
            QueueNotice(text: problem, color: p.dropped) { EmptyView() }
        }
    }

    @ViewBuilder private func group(_ title: String, _ list: [QueueItem], count: Bool) -> some View {
        if !list.isEmpty {
            SectionTitle(text: count ? "\(title) \(list.count)" : title)
            Card {
                ForEach(Array(list.enumerated()), id: \.element.id) { i, item in
                    QueueLine(item: item, divider: i < list.count - 1)
                }
            }
        }
    }
}

/// One change: cover, title, what changes (and the service's words), its state or its actions.
private struct QueueLine: View {
    @Environment(\.palette) private var p
    @EnvironmentObject private var model: AppModel
    let item: QueueItem
    let divider: Bool

    var body: some View {
        VStack(spacing: 0) {
            HStack(alignment: .top, spacing: 12) {
                CoverTile(id: item.mediaId, initials: "", url: item.coverUrl, width: 36, height: 52, radius: 6)
                VStack(alignment: .leading, spacing: 3) {
                    Text(item.title).font(Inter.medium(15)).foregroundStyle(p.text).lineLimit(1)
                    HStack(spacing: 6) {
                        Text(item.what).font(Inter.regular(13)).monospacedDigit().foregroundStyle(p.dim)
                        if item.edits > 1 && item.state != .sent { Text("(\(item.edits) edits merged)").font(Inter.regular(12)).foregroundStyle(p.faint) }
                    }
                    if let e = item.error, item.state == .failed || item.state == .waiting {
                        Text(item.state == .failed ? e : "\(e); Kotora tries again by itself")
                            .font(Inter.regular(12)).foregroundStyle(item.state == .failed ? p.dropped : p.faint).lineLimit(2)
                    }
                    Text(ago(item.sentAt ?? item.editedAt, nowSeconds())).font(Inter.regular(12)).foregroundStyle(p.faint)
                }
                Spacer(minLength: 4)
                end
            }
            .padding(.horizontal, 16).padding(.vertical, 10)
            .opacity(item.state == .sent ? 0.6 : 1)
            if divider { Rectangle().fill(p.border).frame(height: 1).padding(.leading, 64) }
        }
    }

    @ViewBuilder private var end: some View {
        switch item.state {
        case .waiting where item.removed, .failed where item.removed:
            HStack(spacing: 6) {
                if item.state == .waiting { QueuePill(text: "Waiting", color: p.onHold) }
                QueueButton(title: "Undo") { await model.queue(.discard, item) }
            }
        case .failed:
            QueueButton(title: "Discard") { await model.queue(.discard, item) }
        case .sent:
            Label("Sent", systemImage: "checkmark").font(Inter.medium(12)).foregroundStyle(p.watching)
        case .sending:
            QueuePill(text: "Sending", color: p.dim)
        default:
            QueuePill(text: "Waiting", color: p.onHold)
        }
    }
}

/// A conflict: the user's change and the service's entry side by side, Keep mine / Take theirs.
struct ConflictCard: View {
    @Environment(\.palette) private var p
    @EnvironmentObject private var model: AppModel
    let item: QueueItem

    var body: some View {
        let service = item.service.label
        let now = nowSeconds()
        let (theirsHead, theirs, take): (String, String, String) = {
            guard let t = item.theirs else { return (service, "Removed from your list", "Remove here too") }
            let head = "\(service) · changed \(ago(item.theirsChangedAt ?? now, now))"
            return item.removed ? (head, t, "Keep it") : (head, t, "Take theirs")
        }()
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 10) {
                CoverTile(id: item.mediaId, initials: "", url: item.coverUrl, width: 36, height: 52, radius: 6)
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 6) {
                        Image(systemName: "exclamationmark.triangle.fill").font(.system(size: 13)).foregroundStyle(p.accent)
                        Text("Conflict: \(item.title)").font(Inter.semibold(15)).foregroundStyle(p.text).lineLimit(2)
                    }
                    Text(item.theirsEntry.map { "\(service) has \($0)" } ?? "removed on another device")
                        .font(Inter.regular(12)).foregroundStyle(p.faint).lineLimit(2)
                }
            }
            Text(item.removed ? "\(service) changed after you removed it. Kotora did not delete it. Pick one."
                              : "\(service) changed after your edit. Kotora did not overwrite it. Pick one.")
                .font(Inter.regular(13)).foregroundStyle(p.dim).fixedSize(horizontal: false, vertical: true)
            side(head: "Yours · \(item.removed ? "removed" : "edited") here, \(ago(item.editedAt, now))", value: item.mine, on: true,
                 button: QueueButton(title: item.removed ? "Remove anyway" : "Keep mine", primary: true) { await model.queue(.keepMine, item) })
            side(head: theirsHead, value: theirs, on: false,
                 button: QueueButton(title: take) { await model.queue(.takeTheirs, item) })
            if !item.removed {
                Text("When only the progress differs and yours is higher, Kotora sends yours by itself.")
                    .font(Inter.regular(12)).foregroundStyle(p.faint).fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(16)
        .background(RoundedRectangle(cornerRadius: 14).fill(p.surface))
        .overlay(RoundedRectangle(cornerRadius: 14).stroke(p.accent.opacity(0.45), lineWidth: 1))
        .padding(.horizontal, 16)
        .accessibilityIdentifier("conflict/\(item.id)")
    }

    private func side(head: String, value: String, on: Bool, button: QueueButton) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(head).font(Inter.regular(12)).foregroundStyle(p.faint)
            Text(value).font(Inter.medium(16)).monospacedDigit().foregroundStyle(p.text).fixedSize(horizontal: false, vertical: true)
            button.padding(.top, 2)
        }
        .padding(12).frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 10).fill(p.field))
        .overlay(RoundedRectangle(cornerRadius: 10).stroke(on ? p.accent.opacity(0.5) : p.border, lineWidth: 1))
    }
}

private struct QueueButton: View {
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
        .accessibilityIdentifier("queue/\(title)")
    }
}

/// A notice above the queue, with its action, as the desktop's alerts.
private struct QueueNotice<Action: View>: View {
    @Environment(\.palette) private var p
    let text: String
    let color: Color
    @ViewBuilder let action: () -> Action

    var body: some View {
        HStack(spacing: 12) {
            Text(text).font(Inter.regular(13)).foregroundStyle(p.text).fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)
            action()
        }
        .padding(12)
        .background(RoundedRectangle(cornerRadius: 10).fill(color.opacity(0.1)))
        .overlay(RoundedRectangle(cornerRadius: 10).stroke(color.opacity(0.35), lineWidth: 1))
        .padding(.horizontal, 16).padding(.top, 12)
    }
}

private struct QueuePill: View {
    let text: String
    let color: Color

    var body: some View {
        Text(text).font(Inter.medium(11.5)).foregroundStyle(color)
            .padding(.horizontal, 8).frame(height: 20)
            .background(Capsule().fill(color.opacity(0.12)))
    }
}

/// Settings' line to the queue: what waits, and the conflicts that want a pick.
struct QueueLink: View {
    @Environment(\.palette) private var p
    @EnvironmentObject private var model: AppModel
    let waiting: UInt32
    @State private var conflicts: UInt32 = 0
    @State private var open = false

    var body: some View {
        Button { open = true } label: {
            ListLine(title: "Sync queue", subtitle: subtitle, divider: false) { EmptyView() } trailing: {
                HStack(spacing: 8) {
                    if conflicts > 0 { QueuePill(text: conflicts == 1 ? "1 conflict" : "\(conflicts) conflicts", color: p.accent) }
                    Image(systemName: "chevron.right").font(.system(size: 13, weight: .semibold)).foregroundStyle(p.faint)
                }
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier("settings/queue")
        .task(id: model.list.version) { conflicts = await model.editing?.conflictCount() ?? 0 }
        .sheet(isPresented: $open) {
            NavigationStack {
                QueueScreen().toolbar {
                    ToolbarItem(placement: .cancellationAction) { Button("Done") { open = false } }
                }
            }
            .environment(\.palette, p)
            .environmentObject(model)
        }
    }

    private var subtitle: String {
        waiting == 0 ? "Everything is sent." : "\(waiting) waiting to sync"
    }
}
