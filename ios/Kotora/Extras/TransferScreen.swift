import SwiftUI
import UIKit
import UniformTypeIdentifiers

/// Settings → Your list: Export anime/manga list… (a MyAnimeList XML file, saved through the
/// document picker) and Import anime/manga list… (a picked .xml / .xml.gz, previewed, then queued).
/// The desktop's rows and words (`app/src/transfer.rs`); the work is `mobile/src/xml.rs`.
struct TransferSection: View {
    @Environment(\.palette) private var p
    @EnvironmentObject private var model: AppModel

    private var kotora: Kotora? { (model.repo as? CoreRepository)?.kotora }

    var body: some View {
        if let k = kotora, model.session.active != nil {
            SectionTitle(text: "Your list")
            Card {
                ExportLine(kotora: k, kind: .anime, divider: true)
                ImportLine(kotora: k, kind: .anime, divider: model.mangaEnabled)
                if model.mangaEnabled {
                    ExportLine(kotora: k, kind: .manga, divider: true)
                    ImportLine(kotora: k, kind: .manga, divider: false)
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------- export

private struct ExportLine: View {
    @Environment(\.palette) private var p
    let kotora: Kotora
    let kind: MediaKind
    let divider: Bool

    @State private var note: String?
    @State private var failed = false
    @State private var file: ExportFile?

    var body: some View {
        let row = kotora.xmlExportRow(kind: kind)
        Button { export() } label: {
            ListLine(title: row.title, subtitle: note ?? row.note, divider: divider) { EmptyView() } trailing: {
                Text("Export…").font(Inter.medium(14)).foregroundStyle(row.enabled ? p.accentHi : p.faint)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .disabled(!row.enabled)
        .accessibilityIdentifier(kind == .manga ? "set/export-manga" : "set/export")
        .sheet(item: $file) { f in
            ExportPicker(url: f.url) { saved in
                file = nil
                if let saved { note = f.export.savedNote(name: saved.lastPathComponent) }
                try? FileManager.default.removeItem(at: f.url)
            }
            .ignoresSafeArea()
        }
    }

    private func export() {
        let k = kotora, kind = kind
        Task {
            let r = await Task.detached { Result { try k.xmlExport(kind: kind) } }.value
            switch r {
            case .success(let ex):
                let dir = FileManager.default.temporaryDirectory.appendingPathComponent("export-\(UUID().uuidString)", isDirectory: true)
                do {
                    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
                    let url = dir.appendingPathComponent(ex.fileName())
                    try ex.bytes().write(to: url, options: .atomic)
                    file = ExportFile(url: url, export: ex)
                } catch {
                    note = "Can’t write \(ex.fileName()): \(error.localizedDescription)"
                }
            case .failure(let e):
                note = message(of: e)
            }
        }
    }
}

private struct ExportFile: Identifiable {
    let url: URL
    let export: XmlExport
    var id: URL { url }
}

/// The document picker in export mode: the user picks where the file goes (Files, iCloud Drive, …).
private struct ExportPicker: UIViewControllerRepresentable {
    let url: URL
    let done: (URL?) -> Void

    func makeUIViewController(context: Context) -> UIDocumentPickerViewController {
        let vc = UIDocumentPickerViewController(forExporting: [url], asCopy: true)
        vc.delegate = context.coordinator
        return vc
    }

    func updateUIViewController(_ vc: UIDocumentPickerViewController, context: Context) {}

    func makeCoordinator() -> Coordinator { Coordinator(done: done) }

    final class Coordinator: NSObject, UIDocumentPickerDelegate {
        let done: (URL?) -> Void
        init(done: @escaping (URL?) -> Void) { self.done = done }
        func documentPicker(_ c: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) { done(urls.first) }
        func documentPickerWasCancelled(_ c: UIDocumentPickerViewController) { done(nil) }
    }
}

// ---------------------------------------------------------------------------------------- import

private struct ImportLine: View {
    @Environment(\.palette) private var p
    @EnvironmentObject private var model: AppModel
    let kotora: Kotora
    let kind: MediaKind
    let divider: Bool

    @State private var picking = false
    @State private var job: ImportJob?

    var body: some View {
        let title = kind == .manga ? "Import manga list…" : "Import anime list…"
        let note = (try? kotora.xmlImport(kind: kind).row().note) ?? "Sign in first: the file is compared with your list."
        Button { picking = true } label: {
            ListLine(title: title, subtitle: note, divider: divider) { EmptyView() } trailing: {
                Text("Import…").font(Inter.medium(14)).foregroundStyle(p.accentHi)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier(kind == .manga ? "set/import-manga" : "set/import")
        .fileImporter(isPresented: $picking, allowedContentTypes: [.xml, .gzip, .data]) { r in
            guard case .success(let url) = r else { return }
            let scoped = url.startAccessingSecurityScopedResource()
            defer { if scoped { url.stopAccessingSecurityScopedResource() } }
            job = ImportJob(name: url.lastPathComponent, data: (try? Data(contentsOf: url)))
        }
        .sheet(item: $job) { j in
            ImportSheet(kotora: kotora, kind: kind, file: j)
                .environment(\.palette, p)
                .environmentObject(model)
        }
    }
}

private struct ImportJob: Identifiable {
    let id = UUID()
    let name: String
    let data: Data?
}

/// The desktop's import panel: reading, the preview (counts, groups, status to pick), then the
/// queued result with Open Queue and Undo import.
private struct ImportSheet: View {
    @Environment(\.palette) private var p
    @Environment(\.dismiss) private var dismiss
    @EnvironmentObject private var model: AppModel
    let kotora: Kotora
    let kind: MediaKind
    let file: ImportJob

    @State private var job: XmlImport?
    @State private var busy: String?
    @State private var error: String?
    @State private var preview: ImportPreview?
    @State private var useFile: Set<UInt32> = []
    @State private var applying = false
    @State private var done: ImportDone?
    @State private var undone: String?

    /// The most rows listed per group, and conflicts one by one (the desktop's).
    private let shown = 6
    private let conflictsShown = 8

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    if let busy {
                        HStack(spacing: 10) {
                            ProgressView().tint(p.accent)
                            Text(busy).font(Inter.regular(15)).foregroundStyle(p.dim)
                        }
                    }
                    if let error {
                        Text(error).font(Inter.regular(13)).foregroundStyle(p.dropped).accessibilityIdentifier("set/import-error")
                    }
                    if let preview { previewView(preview) }
                    if let done { doneView(done) }
                }
                .padding(16)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .background(p.bg)
            .navigationTitle(kind == .manga ? "Import manga list" : "Import anime list")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(done == nil ? "Cancel" : "Close") { job?.cancel(); dismiss() }.foregroundStyle(p.accentHi)
                }
            }
        }
        .task { await read() }
    }

    @ViewBuilder private func previewView(_ pv: ImportPreview) -> some View {
        HStack {
            Text(pv.file).font(Inter.medium(15)).foregroundStyle(p.text).lineLimit(1).truncationMode(.middle)
            Spacer()
            Text(pv.total).font(Inter.regular(13)).monospacedDigit().foregroundStyle(p.faint)
        }
        HStack(spacing: 6) {
            pill(pv.new, "new", p.watching)
            pill(pv.different, "different", p.onHold)
            pill(pv.same, "same", p.completed)
            pill(pv.unknown, "unknown", p.faint)
        }
        .accessibilityIdentifier("set/import-counts")
        ForEach(pv.groups.filter { $0.heading != "Left out" }, id: \.heading) { group($0) }
        if !pv.conflicts.isEmpty { conflicts(pv.conflicts) }
        ForEach(pv.groups.filter { $0.heading == "Left out" }, id: \.heading) { group($0) }
        Text(pv.note).font(Inter.regular(12)).foregroundStyle(p.faint)
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                button(pv.addNew, enabled: pv.addNewEnabled && !applying) { apply(newOnly: true) }
                button(pv.addAndUpdate, primary: true, enabled: pv.addAndUpdateEnabled && !applying) { apply(newOnly: false) }
            }
            button("Cancel", enabled: !applying) { job?.cancel(); dismiss() }
        }
    }

    @ViewBuilder private func conflicts(_ cs: [ImportConflict]) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("STATUS TO PICK (WITH “UPDATE”)").font(Inter.semibold(11)).tracking(0.4).foregroundStyle(p.faint)
            ForEach(cs.prefix(conflictsShown), id: \.id) { c in
                VStack(alignment: .leading, spacing: 6) {
                    Text(c.title).font(Inter.regular(13.5)).foregroundStyle(p.text).lineLimit(1)
                    HStack(spacing: 6) {
                        choice(c.keep, on: !useFile.contains(c.id)) { useFile.remove(c.id) }
                        choice(c.useFile, on: useFile.contains(c.id)) { useFile.insert(c.id) }
                    }
                }
            }
            if cs.count > 1 {
                let more = cs.count > conflictsShown ? "and \(cs.count - conflictsShown) more. " : ""
                Text("\(more)All of them:").font(Inter.regular(13)).foregroundStyle(p.faint)
                HStack(spacing: 6) {
                    choice("Keep all yours", on: useFile.isEmpty) { useFile = [] }
                    choice("Use all from the file", on: cs.allSatisfy { useFile.contains($0.id) }) { useFile = Set(cs.map(\.id)) }
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(12)
        .background(RoundedRectangle(cornerRadius: 10).fill(p.field))
        .overlay(RoundedRectangle(cornerRadius: 10).stroke(p.border, lineWidth: 1))
        .accessibilityIdentifier("set/import-conflicts")
    }

    @ViewBuilder private func doneView(_ d: ImportDone) -> some View {
        Text(undone ?? d.message).font(Inter.regular(15)).foregroundStyle(p.text).accessibilityIdentifier("set/import-done")
        if d.canUndo && undone == nil {
            HStack(spacing: 8) {
                NavigationLink {
                    QueueScreen()
                } label: {
                    label("Open Queue")
                }
                .buttonStyle(.borderless)
                button("Undo import", enabled: !applying) { undo() }
            }
        }
        button("Close") { dismiss() }
    }

    private func group(_ g: ImportGroup) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(g.heading.uppercased()).font(Inter.semibold(11)).tracking(0.4).foregroundStyle(p.faint)
            ForEach(Array(g.rows.prefix(shown).enumerated()), id: \.offset) { Text($0.element).font(Inter.regular(13.5)).foregroundStyle(p.dim).lineLimit(1) }
            if g.rows.count > shown {
                Text("and \(g.rows.count - shown) more").font(Inter.regular(13)).foregroundStyle(p.faint)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(12)
        .background(RoundedRectangle(cornerRadius: 10).fill(p.field))
        .overlay(RoundedRectangle(cornerRadius: 10).stroke(p.border, lineWidth: 1))
    }

    private func pill(_ n: UInt32, _ word: String, _ color: Color) -> some View {
        HStack(spacing: 5) {
            Circle().fill(color).frame(width: 6, height: 6)
            Text("\(n) \(word)").font(Inter.medium(12)).monospacedDigit().foregroundStyle(p.text)
        }
        .padding(.horizontal, 8).frame(height: 22)
        .background(Capsule().fill(color.opacity(0.14)))
    }

    private func choice(_ title: String, on: Bool, _ action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Text(title).font(Inter.medium(12.5)).foregroundStyle(on ? p.onAccent : p.text)
                .padding(.horizontal, 10).frame(height: 28)
                .background(RoundedRectangle(cornerRadius: 7).fill(on ? p.accent : p.raised))
        }
        .buttonStyle(.borderless)
    }

    private func label(_ title: String, primary: Bool = false, enabled: Bool = true) -> some View {
        Text(title).font(Inter.medium(14)).foregroundStyle(primary ? p.onAccent : p.text)
            .padding(.horizontal, 14).frame(height: 36)
            .background(RoundedRectangle(cornerRadius: 9).fill(primary ? p.accent : p.raised))
            .overlay(RoundedRectangle(cornerRadius: 9).stroke(primary ? .clear : p.borderStrong, lineWidth: 1))
            .opacity(enabled ? 1 : 0.45)
    }

    private func button(_ title: String, primary: Bool = false, enabled: Bool = true, _ action: @escaping () -> Void) -> some View {
        Button(action: action) { label(title, primary: primary, enabled: enabled) }
            .buttonStyle(.borderless)
            .disabled(!enabled)
    }

    // ---- the work: blocking calls on a worker ----

    private func read() async {
        guard job == nil else { return }
        guard let data = file.data else {
            error = "Can’t read the file: \(file.name)"
            return
        }
        do {
            let j = try kotora.xmlImport(kind: kind)
            job = j
            busy = "Reading the file…"
            let poll = Task {
                while !Task.isCancelled {
                    let b = j.busy()
                    if !b.isEmpty { busy = b }
                    try? await Task.sleep(for: .milliseconds(150))
                }
            }
            let name = file.name
            let r = await Task.detached { Result { try j.read(bytes: data, fileName: name) } }.value
            poll.cancel()
            busy = nil
            switch r {
            case .success(let pv): preview = pv
            case .failure(let e): error = message(of: e)
            }
        } catch {
            self.error = message(of: error)
        }
    }

    private func apply(newOnly: Bool) {
        guard let j = job else { return }
        applying = true
        let picks = Array(useFile)
        Task {
            let r = await Task.detached { Result { try j.apply(newOnly: newOnly, useFile: picks) } }.value
            applying = false
            switch r {
            case .success(let d):
                preview = nil
                done = d
                if d.queued > 0 { model.sync() }
            case .failure(let e): error = message(of: e)
            }
        }
    }

    private func undo() {
        guard let j = job else { return }
        applying = true
        Task {
            let r = await Task.detached { Result { try j.undo() } }.value
            applying = false
            switch r {
            case .success(let m):
                undone = m
                await model.reload()
            case .failure(let e): error = message(of: e)
            }
        }
    }
}
