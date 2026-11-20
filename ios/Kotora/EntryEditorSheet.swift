import SwiftUI

/// The editor of one entry, as the desktop's: status, progress (and a manga's volumes), score,
/// the rewatch (reread) count, then Cancel / Save (one change, to the sync queue first). The rules
/// of the − / + and the status cells are the core's (`editorStep`, `editorSetStatus`).
struct EntryEditorSheet: View {
    @Environment(\.palette) private var p
    @Environment(\.dismiss) private var dismiss
    @EnvironmentObject private var model: AppModel
    let ref: RowRef
    @State private var ed: EntryEditor?
    @State private var scale: ScoreScale?
    @State private var saving = false
    @State private var problem: String?

    var body: some View {
        NavigationStack {
            Group {
                if let ed { form(ed) } else if let problem { Text(problem).font(Inter.regular(15)).foregroundStyle(p.dropped).padding(24) } else { ProgressView().tint(p.accent) }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(p.bg)
            .navigationTitle(ed?.title ?? "")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() }.foregroundStyle(p.dim) }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") {
                        guard let ed else { return }
                        saving = true
                        Task {
                            if await model.saveEditor(ed) { dismiss() }
                            saving = false
                        }
                    }
                    .font(Inter.semibold(16)).foregroundStyle(p.accent).disabled(ed == nil || saving)
                    .accessibilityIdentifier("editor/save")
                }
            }
        }
        .task {
            guard let e = model.editing else { return }
            do {
                ed = try await e.editor(kind: ref.kind, id: ref.id)
                scale = try await e.scoreScale()
            } catch {
                problem = message(of: error)
            }
        }
    }

    private func form(_ e: EntryEditor) -> some View {
        let manga = e.kind == .manga
        return ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                SectionTitle(text: "Status")
                Card {
                    ForEach(Array(ListStatus.allCases.enumerated()), id: \.element) { i, st in
                        Button { ed = editorSetStatus(editor: e, status: st) } label: {
                            HStack(spacing: 10) {
                                Circle().fill(p.status(st)).frame(width: 8, height: 8)
                                Text(st.label(e.kind)).font(Inter.regular(15)).foregroundStyle(p.text)
                                Spacer()
                                if e.status == st { Image(systemName: "checkmark").font(.system(size: 14, weight: .semibold)).foregroundStyle(p.accent) }
                            }
                            .padding(.horizontal, 16).frame(height: 46).contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        if i < ListStatus.allCases.count - 1 { Rectangle().fill(p.border).frame(height: 1).padding(.leading, 34) }
                    }
                }

                SectionTitle(text: manga ? "Chapters" : "Progress")
                Card {
                    stepper(manga ? "Chapters" : "Episodes", value: e.progress, total: e.total,
                            minus: { ed = editorStep(editor: e, delta: -1) }, plus: { ed = editorStep(editor: e, delta: 1) }, id: "progress")
                    if manga {
                        Rectangle().fill(p.border).frame(height: 1).padding(.leading, 16)
                        stepper("Volumes", value: e.volumes, total: e.totalVolumes,
                                minus: { ed = editorStepVolumes(editor: e, delta: -1) }, plus: { ed = editorStepVolumes(editor: e, delta: 1) }, id: "volumes")
                    }
                }

                SectionTitle(text: "Score", trailing: model.editing?.scoreLabel(e.score))
                Card { scoreCells(e) }

                // As the desktop's details: the count shows for a completed entry, or one counted before.
                if e.status == .completed || e.timesRewatched > 0 || e.rewatching {
                    SectionTitle(text: manga ? "Reread" : "Rewatched")
                    Card {
                        stepper(manga ? "Times reread" : "Times rewatched", value: e.timesRewatched, total: nil,
                                minus: { if e.timesRewatched > 0 { ed?.timesRewatched -= 1 } }, plus: { ed?.timesRewatched += 1 }, id: "times")
                    }
                    if e.rewatching {
                        Text(manga ? "Rereading now: the last chapter ends the reread and counts it." : "Rewatching now: the last episode ends the rewatch and counts it.")
                            .font(Inter.regular(12)).foregroundStyle(p.faint).padding(.horizontal, 20).padding(.top, 6)
                    }
                }

                Text("It goes to the sync queue first, then to \(model.session.active?.service.label ?? "the service").")
                    .font(Inter.regular(12)).foregroundStyle(p.faint).padding(.horizontal, 20).padding(.top, 14).padding(.bottom, 32)
            }
        }
    }

    private func stepper(_ label: String, value: UInt32, total: UInt32?, minus: @escaping () -> Void, plus: @escaping () -> Void, id: String) -> some View {
        HStack(spacing: 12) {
            Text(label).font(Inter.regular(15)).foregroundStyle(p.text)
            Spacer()
            round("minus", action: minus).disabled(value == 0).accessibilityIdentifier("editor/\(id)/minus")
            Text(total.map { "\(value) / \($0)" } ?? "\(value)")
                .font(Inter.medium(16)).monospacedDigit().foregroundStyle(p.text).frame(minWidth: 72)
                .accessibilityIdentifier("editor/\(id)")
            round("plus", action: plus).disabled(total.map { value >= $0 } ?? false).accessibilityIdentifier("editor/\(id)/plus")
        }
        .padding(.horizontal, 16).frame(height: 58)
    }

    private func round(_ symbol: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: symbol).font(.system(size: 15, weight: .semibold)).foregroundStyle(p.text)
                .frame(width: 40, height: 40)
                .background(Circle().fill(p.raised))
                .overlay(Circle().stroke(p.borderStrong, lineWidth: 1))
        }
        .buttonStyle(.borderless)
    }

    @ViewBuilder private func scoreCells(_ e: EntryEditor) -> some View {
        let choices = scale?.choices ?? []
        let shown = model.editing?.scoreLabel(e.score)
        LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 8), count: choices.count <= 5 ? choices.count + 1 : 6), spacing: 8) {
            cell("–", on: e.score == nil, id: "none") { ed?.score = nil }
            ForEach(choices, id: \.points) { c in
                cell(c.label, on: e.score != nil && shown == c.label, id: String(c.points)) { ed?.score = c.points }
            }
        }
        .padding(12)
        if scale?.fine == true {
            Rectangle().fill(p.border).frame(height: 1).padding(.leading, 16)
            HStack(spacing: 12) {
                Text("Fine").font(Inter.regular(15)).foregroundStyle(p.text)
                Spacer()
                round("minus") { ed?.score = model.editing?.scoreStep(e.score, -1) }
                Text(shown ?? "").font(Inter.medium(16)).monospacedDigit().foregroundStyle(p.text).frame(minWidth: 72)
                round("plus") { ed?.score = model.editing?.scoreStep(e.score, 1) }
            }
            .padding(.horizontal, 16).frame(height: 58)
        }
    }

    private func cell(_ label: String, on: Bool, id: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Text(label).font(Inter.medium(14)).monospacedDigit().foregroundStyle(on ? p.onAccent : p.text)
                .lineLimit(1).minimumScaleFactor(0.6)
                .frame(maxWidth: .infinity).frame(height: 40)
                .background(RoundedRectangle(cornerRadius: 8).fill(on ? p.accent : p.field))
                .overlay(RoundedRectangle(cornerRadius: 8).stroke(on ? .clear : p.borderStrong, lineWidth: 1))
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier("editor/score/\(id)")
    }
}
