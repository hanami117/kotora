import SwiftUI

/// One entry, as the desktop's details page: the header and the "Your list" card from the saved
/// list, then the service's information (stats, other titles, Synopsis, Information). A title not
/// on the list (from Search, Airing, a notification, a pasted id) gets the add buttons instead.
struct DetailsScreen: View {
    @Environment(\.palette) private var p
    @EnvironmentObject private var model: AppModel
    let ref: RowRef
    @State private var row: Row?
    @State private var loaded = false
    @StateObject private var info = DetailsInfo()

    private var service: String { model.session.active?.service.label ?? "MyAnimeList" }

    var body: some View {
        Group {
            if let row {
                content(row)
            } else if loaded {
                notOnList
            } else {
                ProgressView().tint(p.accent)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(p.bg)
        .toolbarBackground(.hidden, for: .navigationBar)
        .overlay(alignment: .bottom) {
            if let snack = model.snack { SnackBar(snack: snack).transition(.move(edge: .bottom).combined(with: .opacity)) }
        }
        // Read again whenever the list changes (a +1 here or a sync).
        .task(id: model.list.version) {
            row = await model.entry(ref.kind, ref.id)
            loaded = true
        }
        .task(id: ref) { await info.load(model, kind: ref.kind, id: ref.id) }
    }

    /// The details of a title that is not on the list: the service's information and the add.
    private var notOnList: some View {
        let i = info.info
        let title = i?.title ?? ref.stub?.title ?? ""
        let cover = i?.coverUrl ?? ref.stub?.coverUrl
        let stub = TitleStub(kind: ref.kind, id: ref.id, title: title, coverUrl: cover,
                             format: i?.format ?? ref.stub?.format ?? "", episodes: i?.episodes ?? ref.stub?.episodes)
        return ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                hero(id: ref.id, title: title, titleEn: nil, meta: "", cover: cover, initials: title.initialsOfTitle)
                if let i {
                    tagsRow(i)
                    DetailsStats(info: i).padding(.bottom, 12)
                    DetailsTitles(info: i, shown: [title])
                }
                HStack(spacing: 8) {
                    ForEach(model.serviceLinks(ref.kind, ref.id), id: \.url) { OpenOnButton(label: $0.label, url: $0.url) }
                }
                .padding(.horizontal, 16).padding(.bottom, 12)
                AddToListCard(kind: ref.kind, service: service) { model.addTitle(stub, $0) }
                    .padding(.bottom, 16)
                DetailsBody(state: info, service: service) { Task { await info.load(model, kind: ref.kind, id: ref.id) } }
                Spacer(minLength: 80)
            }
        }
        .ignoresSafeArea(edges: .top)
    }

    /// The service's tags: type, episodes, airing (lit), season.
    private func tagsRow(_ i: ServiceDetails) -> some View {
        FlowLayout(spacing: 6) {
            ForEach(i.tags, id: \.self) { t in
                Tag(text: t, style: i.running && (t == "Airing" || t == "Publishing") ? .air : .plain)
            }
        }
        .padding(.horizontal, 16).padding(.top, 16).padding(.bottom, 12)
    }

    private func content(_ row: Row) -> some View {
        let manga = ref.kind == .manga
        return ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                hero(row)
                HStack(spacing: 8) {
                    if let (text, live) = row.airPill { pill(text, color: live ? p.air : p.dim, dot: live) }
                    if row.waiting { pill("Waiting to sync", color: p.onHold, dot: false) }
                }
                .padding(.horizontal, 16).padding(.top, 16).padding(.bottom, 12)
                HStack(spacing: 8) {
                    ForEach(model.serviceLinks(ref.kind, ref.id), id: \.url) { OpenOnButton(label: $0.label, url: $0.url) }
                }
                .padding(.horizontal, 16).padding(.bottom, 12)
                .accessibilityIdentifier("detailsOpenOn")
                if let i = info.info {
                    DetailsStats(info: i).padding(.bottom, 12)
                    DetailsTitles(info: i, shown: [row.title, row.titleEn ?? ""])
                }

                Card {
                    HStack {
                        Text("Your list").font(Inter.semibold(15)).foregroundStyle(p.text)
                        Spacer()
                        if let a = model.session.active { Text(a.service.label).font(Inter.regular(12)).foregroundStyle(p.faint) }
                    }
                    .padding(.horizontal, 16).padding(.top, 14).padding(.bottom, 4)
                    HStack(spacing: 10) {
                        field("Status") {
                            Circle().fill(p.status(row.status)).frame(width: 8, height: 8)
                            Text(row.status.label(ref.kind))
                        }
                        field("Score") { Text("★ \(row.score ?? "–")") }
                    }
                    .padding(.horizontal, 16).padding(.vertical, 10)
                    VStack(alignment: .leading, spacing: 6) {
                        Text(manga ? "Chapters" : "Episodes").font(Inter.regular(12)).foregroundStyle(p.faint)
                        HStack(spacing: 10) {
                            Text("\(row.progress) of \(row.total.map(String.init) ?? "?")")
                                .font(Inter.medium(15)).monospacedDigit().foregroundStyle(p.text)
                                .padding(.horizontal, 14).frame(height: 44)
                                .background(RoundedRectangle(cornerRadius: 8).fill(p.field))
                                .overlay(RoundedRectangle(cornerRadius: 8).stroke(p.borderStrong, lineWidth: 1))
                            Button { model.plusOne(row) } label: {
                                Text("+1 \(ref.kind.unit.capitalized) \(row.progress + 1)")
                                    .font(Inter.semibold(15)).foregroundStyle(row.canPlusOne ? p.onAccent : p.faint)
                                    .frame(maxWidth: .infinity).frame(height: 44)
                                    .background(RoundedRectangle(cornerRadius: 10).fill(row.canPlusOne ? p.accent : p.raised))
                            }
                            .disabled(!row.canPlusOne)
                            .accessibilityLabel("Plus one \(ref.kind.unit)")
                        }
                        if manga {
                            Text("Volumes \(row.volumes) of \(row.totalVolumes.map(String.init) ?? "?")")
                                .font(Inter.regular(12)).foregroundStyle(p.faint).padding(.top, 4)
                        }
                    }
                    .padding(.horizontal, 16).padding(.top, 2).padding(.bottom, 14)
                    EntryActions(row: row)
                }
                DetailsBody(state: info, service: service) { Task { await info.load(model, kind: ref.kind, id: ref.id) } }
                    .padding(.top, 16)
                Spacer(minLength: 80)
            }
        }
        .ignoresSafeArea(edges: .top)
    }

    private func hero(_ row: Row) -> some View {
        hero(id: row.id, title: row.title, titleEn: row.titleEn, meta: row.meta, cover: row.coverUrl, initials: row.initials)
    }

    private func hero(id: UInt32, title: String, titleEn: String?, meta: String, cover: String?, initials: String) -> some View {
        ZStack(alignment: .bottomLeading) {
            CoverTile(id: id, initials: "", url: cover, width: nil, height: 250, radius: 0)
                .blur(radius: 30).opacity(0.55)
                .clipped()
                .overlay(LinearGradient(stops: [.init(color: p.bg.opacity(0), location: 0.3), .init(color: p.bg, location: 1)], startPoint: .top, endPoint: .bottom))
            HStack(alignment: .bottom, spacing: 16) {
                CoverTile(id: id, initials: initials, url: cover, width: 120, height: 170, radius: 12)
                    .shadow(color: .black.opacity(0.35), radius: 10, y: 4)
                VStack(alignment: .leading, spacing: 4) {
                    Text(title).font(Inter.semibold(22)).foregroundStyle(p.text).fixedSize(horizontal: false, vertical: true)
                    if let en = titleEn, en != title { Text(en).font(Inter.regular(13)).foregroundStyle(p.dim) }
                    if !meta.isEmpty { Text(meta).font(Inter.regular(12)).foregroundStyle(p.faint).padding(.top, 2) }
                }
                .padding(.bottom, 4)
            }
            .padding(.horizontal, 16).offset(y: 80)
        }
        .frame(height: 250)
        .padding(.bottom, 80)
    }

    private func pill(_ text: String, color: Color, dot: Bool) -> some View {
        HStack(spacing: 6) {
            if dot { Circle().fill(color).frame(width: 8, height: 8) }
            Text(text).font(Inter.regular(12)).foregroundStyle(color)
        }
        .padding(.horizontal, 12).frame(height: 30)
        .background(Capsule().fill(p.surface))
        .overlay(Capsule().stroke(color == p.dim ? p.border : color.opacity(0.4), lineWidth: 1))
    }

    private func field<C: View>(_ label: String, @ViewBuilder _ c: () -> C) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(label).font(Inter.regular(12)).foregroundStyle(p.faint)
            HStack(spacing: 8) { c() }
                .font(Inter.regular(14)).foregroundStyle(p.text)
                .padding(.horizontal, 12).frame(maxWidth: .infinity, alignment: .leading).frame(height: 44)
                .background(RoundedRectangle(cornerRadius: 8).fill(p.field))
                .overlay(RoundedRectangle(cornerRadius: 8).stroke(p.borderStrong, lineWidth: 1))
        }
    }
}

extension String {
    /// Two initials for a cover tile, as `Row.initials`.
    var initialsOfTitle: String {
        let words = split(whereSeparator: { $0 == " " || $0 == "-" || $0 == ":" })
            .filter { $0.first.map { $0.isLetter || $0.isNumber } == true }
        return String(words.prefix(2).compactMap(\.first)).uppercased()
    }
}
