import SwiftUI

/// The long-press menu of a row or poster, as the desktop's right-click menu (`ctxmenu.rs`):
/// Move to ▸ the statuses, Score ▸ the user's format and "No score", then the Open on links.
/// Each change goes at once, with Undo on the message.
struct QuickMenuItems: View {
    @EnvironmentObject private var model: AppModel
    let row: Row

    var body: some View {
        Menu("Move to") {
            ForEach(ListStatus.allCases, id: \.self) { st in
                Button { model.moveTo(row, st) } label: {
                    if st == row.status { Label(st.label(row.kind), systemImage: "checkmark") } else { Text(st.label(row.kind)) }
                }
                .accessibilityIdentifier("ctx/move/\(st)")
            }
        }
        Menu("Score") {
            ForEach(model.scoreChoices, id: \.points) { c in
                Button { model.setScore(row, c.points) } label: {
                    if c.label == row.score { Label(c.label, systemImage: "checkmark") } else { Text(c.label) }
                }
            }
            Divider()
            Button { model.setScore(row, nil) } label: {
                if row.score == nil { Label("No score", systemImage: "checkmark") } else { Text("No score") }
            }
        }
        OpenOnMenuItems(kind: row.kind, id: row.id)
    }
}

/// "Mark anyway?" before a +1 of an episode not aired yet (the desktop's `ui.rs` question).
struct MarkAnywayAlert: ViewModifier {
    @EnvironmentObject private var model: AppModel

    func body(content: Content) -> some View {
        content.alert("Mark anyway?", isPresented: Binding(get: { model.markAnyway != nil }, set: { if !$0 { model.markAnyway = nil } }), presenting: model.markAnyway) { m in
            Button("Mark anyway") { model.plusOneNow(m.row) }
            Button("Cancel", role: .cancel) { model.markAnyway = nil }
        } message: { m in
            Text(m.question)
        }
    }
}

/// A sign-in problem while signed in, in full: e.g. another account refused while the old one's
/// changes still wait to be sent.
struct SignInProblemAlert: ViewModifier {
    @EnvironmentObject private var model: AppModel

    func body(content: Content) -> some View {
        content.alert("Couldn\u{2019}t sign in", isPresented: Binding(get: { model.signInProblem != nil }, set: { if !$0 { model.signInProblem = nil } }), presenting: model.signInProblem) { _ in
            Button("OK", role: .cancel) { model.signInProblem = nil }
        } message: { text in
            Text(text)
        }
    }
}
