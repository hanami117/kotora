import SwiftUI

/// The Profile page, as the desktop's (profile.rs): the picture, the name and when they joined,
/// "Open on {service}", the service's numbers (days, episodes, mean score, a stacked bar per
/// status), how the user scores (a 1-10 chart from the saved list) and Sign out.
struct ProfileScreen: View {
    @Environment(\.palette) private var p
    @Environment(\.dismiss) private var dismiss
    @EnvironmentObject private var model: AppModel
    @State private var info: ProfileInfo?

    var body: some View {
        NavigationStack {
            ScrollView {
                if let info, let account = model.session.active {
                    VStack(alignment: .leading, spacing: 16) {
                        head(info, account)
                        NavigationLink { StatsScreen() } label: {
                            HStack(spacing: 12) {
                                Image(systemName: "chart.bar.xaxis").foregroundStyle(p.accent).frame(width: 22)
                                Text("Statistics").font(Inter.medium(15)).foregroundStyle(p.text)
                                Spacer()
                                Image(systemName: "chevron.right").font(.system(size: 13, weight: .semibold)).foregroundStyle(p.faint)
                            }
                            .padding(.horizontal, 16).frame(height: 52)
                            .background(RoundedRectangle(cornerRadius: 14).fill(p.surface))
                            .overlay(RoundedRectangle(cornerRadius: 14).stroke(p.border, lineWidth: 1))
                        }
                        .buttonStyle(.plain)
                        .accessibilityIdentifier("profileStatistics")
                        anime(info)
                        scores(info)
                        HStack(spacing: 12) {
                            Text(info.signedInLine).font(Inter.regular(14)).foregroundStyle(p.dim)
                                .frame(maxWidth: .infinity, alignment: .leading).fixedSize(horizontal: false, vertical: true)
                            Button {
                                dismiss()
                                model.signOut(info.service)
                            } label: {
                                Text("Sign out").font(Inter.medium(13)).foregroundStyle(p.text)
                                    .padding(.horizontal, 14).frame(height: 34)
                                    .background(RoundedRectangle(cornerRadius: 8).fill(p.raised))
                                    .overlay(RoundedRectangle(cornerRadius: 8).stroke(p.borderStrong, lineWidth: 1))
                            }
                            .buttonStyle(.borderless)
                            .disabled(model.isSample)
                        }
                        .padding(.horizontal, 16).padding(.vertical, 12)
                        .overlay(RoundedRectangle(cornerRadius: 14).stroke(p.border, lineWidth: 1))
                        .accessibilityIdentifier("profileAccount")
                    }
                    .padding(16)
                } else {
                    ProgressView().tint(p.accent).padding(40)
                }
            }
            .background(p.bg)
            .navigationTitle("Profile")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } } }
        }
        .task(id: model.session.active?.service) {
            guard let s = model.session.active?.service else { return }
            info = await (model.repo as? AccountRepository)?.profile(s)
        }
    }

    private func head(_ info: ProfileInfo, _ account: Account) -> some View {
        HStack(spacing: 16) {
            Avatar(account: account, size: 64)
            VStack(alignment: .leading, spacing: 4) {
                Text(info.userName).font(Inter.semibold(22)).foregroundStyle(p.text).lineLimit(1)
                Text(info.joinedLine).font(Inter.regular(14)).foregroundStyle(p.dim)
                OpenOnButton(label: info.openLabel, url: info.profileUrl).padding(.top, 6)
                    .accessibilityIdentifier("profileOpen")
            }
            Spacer(minLength: 0)
        }
        .padding(18)
        .background(RoundedRectangle(cornerRadius: 14).fill(p.surface))
        .overlay(RoundedRectangle(cornerRadius: 14).stroke(p.border, lineWidth: 1))
    }

    private func card<C: View>(_ title: String, @ViewBuilder _ c: () -> C) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(title).font(Inter.semibold(15)).foregroundStyle(p.text)
            c()
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 14).fill(p.surface))
        .overlay(RoundedRectangle(cornerRadius: 14).stroke(p.border, lineWidth: 1))
    }

    private func figure(_ big: String, _ small: String) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(big).font(Inter.semibold(24)).monospacedDigit().foregroundStyle(p.text)
            Text(small).font(Inter.regular(12)).foregroundStyle(p.faint)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func anime(_ info: ProfileInfo) -> some View {
        card("Anime") {
            HStack(spacing: 16) {
                figure(info.days.map { String(format: "%.1f", $0) } ?? "–", "Days watched")
                figure(info.episodes.map(grouped) ?? "–", "Episodes")
                figure(info.meanScore.map { String(format: "%.2f", $0) } ?? "–", "Mean score")
            }
            VStack(alignment: .leading, spacing: 6) {
                HStack {
                    Text("By status").font(Inter.regular(12)).foregroundStyle(p.faint)
                    Spacer()
                    Text("\(grouped(info.total)) entries").font(Inter.regular(12)).monospacedDigit().foregroundStyle(p.dim)
                }
                GeometryReader { geo in
                    HStack(spacing: 2) {
                        if info.total == 0 {
                            Rectangle().fill(p.border)
                        } else {
                            let shown = info.counts.filter { $0.count > 0 }
                            let w = geo.size.width - CGFloat(max(shown.count - 1, 0)) * 2
                            ForEach(shown, id: \.status) { c in
                                Rectangle().fill(p.status(c.status)).frame(width: w * CGFloat(c.count) / CGFloat(info.total))
                            }
                        }
                    }
                }
                .frame(height: 12).clipShape(RoundedRectangle(cornerRadius: 6))
            }
            VStack(spacing: 0) {
                ForEach(info.counts, id: \.status) { c in
                    HStack(spacing: 8) {
                        Circle().fill(p.status(c.status)).frame(width: 8, height: 8)
                        Text(c.status.label(.anime)).font(Inter.regular(14)).foregroundStyle(p.dim)
                        Spacer()
                        Text(grouped(c.count)).font(Inter.medium(14)).monospacedDigit().foregroundStyle(p.text)
                    }
                    .padding(.vertical, 3)
                }
            }
            if let line = info.rewatchedLine ?? info.pendingLine {
                Text(line).font(Inter.regular(12)).foregroundStyle(p.faint).fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    private func scores(_ info: ProfileInfo) -> some View {
        let n = info.scoreSpread
        let maxN = max(n.max() ?? 0, 1)
        return card("Your scores") {
            HStack(alignment: .bottom, spacing: 6) {
                ForEach(Array(n.enumerated()), id: \.offset) { i, c in
                    VStack(spacing: 4) {
                        Text(c == 0 ? " " : "\(c)").font(Inter.regular(10)).monospacedDigit().foregroundStyle(p.dim)
                        RoundedRectangle(cornerRadius: 3).fill(c == 0 ? p.border : p.accent)
                            .frame(height: c == 0 ? 2 : max(110 * CGFloat(c) / CGFloat(maxN), 4))
                        Text("\(i + 1)").font(Inter.regular(11)).monospacedDigit().foregroundStyle(p.faint)
                    }
                    .frame(maxWidth: .infinity)
                }
            }
            .frame(height: 164, alignment: .bottom)
            Text(info.scoredLine).font(Inter.regular(12)).foregroundStyle(p.dim)
        }
    }

    private func grouped(_ n: UInt32) -> String {
        let f = NumberFormatter()
        f.numberStyle = .decimal
        f.locale = Locale(identifier: "en_US")
        return f.string(from: NSNumber(value: n)) ?? "\(n)"
    }
}
