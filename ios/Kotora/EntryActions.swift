import SwiftUI

/// The bottom of the details page's "Your list" card, as the desktop's: the rewatch count, then
/// Edit, and Rewatch for a completed entry (Stop rewatching during one). Manga: Reread.
struct EntryActions: View {
    @Environment(\.palette) private var p
    @EnvironmentObject private var model: AppModel
    let row: Row
    @State private var editing = false
    @State private var removeQuestion: String?

    var body: some View {
        let manga = row.kind == .manga
        Rectangle().fill(p.border).frame(height: 1)
        VStack(alignment: .leading, spacing: 12) {
            if row.status == .completed || row.timesRewatched > 0 {
                HStack {
                    Text(manga ? "Reread" : "Rewatched").font(Inter.regular(14)).foregroundStyle(p.faint)
                    Spacer()
                    if row.rewatching { Tag(text: manga ? "Rereading" : "Rewatching") }
                    Text(times(row.timesRewatched)).font(Inter.medium(14)).monospacedDigit().foregroundStyle(p.text)
                        .accessibilityIdentifier("details/rewatched")
                }
            }
            HStack(spacing: 8) {
                button("Edit", primary: true, id: "edit") { editing = true }
                if row.status == .completed && !row.rewatching {
                    button(manga ? "Reread" : "Rewatch", icon: "arrow.counterclockwise", id: "rewatch") { model.rewatch(row, start: true) }
                        .accessibilityHint(manga ? "Reread from chapter 1" : "Rewatch from episode 1")
                } else if row.rewatching {
                    button(manga ? "Stop rereading" : "Stop rewatching", id: "stop-rewatch") { model.rewatch(row, start: false) }
                }
                Spacer()
            }
            HStack(spacing: 8) {
                if row.progress > 0 {
                    button(manga ? "−1 chapter" : "−1 episode", id: "minus-one") { model.minusOne(row) }
                }
                button("Remove from list…", id: "remove") { Task { await ask() } }
                Spacer()
            }
        }
        .padding(16)
        .confirmationDialog(removeQuestion ?? "", isPresented: Binding(get: { removeQuestion != nil }, set: { if !$0 { removeQuestion = nil } }), titleVisibility: .visible) {
            Button("Remove", role: .destructive) { model.removeFromList(row) }
            Button("Cancel", role: .cancel) {}
        }
        .sheet(isPresented: $editing) {
            EntryEditorSheet(ref: RowRef(kind: row.kind, id: row.id))
                .environment(\.palette, p)
                .presentationDetents([.large])
        }
    }

    /// The core's question (with the mirror note when the mirror is on), then the confirm.
    private func ask() async {
        removeQuestion = (try? await model.editing?.removeQuestion(kind: row.kind, id: row.id)) ?? "Remove \(row.title) from your list?"
    }

    /// "Not yet", "once", "twice", "3 times" (the core's words).
    private func times(_ n: UInt32) -> String {
        switch n {
        case 0: return "Not yet"
        case 1: return "Once"
        case 2: return "Twice"
        default: return "\(n) times"
        }
    }

    private func button(_ title: String, icon: String? = nil, primary: Bool = false, id: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack(spacing: 6) {
                if let icon { Image(systemName: icon).font(.system(size: 13, weight: .semibold)).foregroundStyle(primary ? p.onAccent : p.completed) }
                Text(title).font(Inter.medium(14)).foregroundStyle(primary ? p.onAccent : p.text)
            }
            .padding(.horizontal, 16).frame(height: 40)
            .background(RoundedRectangle(cornerRadius: 10).fill(primary ? p.accent : p.raised))
            .overlay(RoundedRectangle(cornerRadius: 10).stroke(primary ? .clear : p.borderStrong, lineWidth: 1))
        }
        .buttonStyle(.borderless)
        .accessibilityIdentifier("details/\(id)")
    }
}
