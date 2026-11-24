import SwiftUI

/// Ways into the Statistics page from anywhere: the profile menu's sheet and Home's "Your stats" card.

/// The Statistics page in its own sheet (the avatar menu, Home's card).
struct StatsSheet: View {
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            StatsScreen()
                .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } } }
        }
    }
}

/// Home's last card: this week from the Statistics page (same number and words), and a link to the page.
struct HomeStatsCard: View {
    @Environment(\.palette) private var p
    @EnvironmentObject private var model: AppModel
    @State private var data: StatsData?
    @State private var open = false

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .lastTextBaseline) {
                Text("Your stats").font(Inter.semibold(17)).foregroundStyle(p.text)
                Spacer()
                HStack(spacing: 4) {
                    Text("See all").font(Inter.medium(13))
                    Image(systemName: "arrow.right").font(.system(size: 11, weight: .semibold))
                }
                .foregroundStyle(p.dim).padding(.vertical, 4)
            }
            if let d = data {
                let w = Words(d.kind)
                VStack(alignment: .leading, spacing: 3) {
                    Text("THIS WEEK").font(Inter.semibold(11)).tracking(0.4).foregroundStyle(p.faint)
                    Text(statsGrouped(n: UInt64(d.week.units))).font(Inter.semibold(22)).monospacedDigit().foregroundStyle(p.text)
                    Text(statsPeriodLine(d.kind, d.week, w)).font(Inter.regular(12.5)).foregroundStyle(p.dim)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            } else {
                Text(" ").font(Inter.regular(13))
            }
        }
        .padding(14)
        .background(RoundedRectangle(cornerRadius: 14).fill(p.surface))
        .overlay(RoundedRectangle(cornerRadius: 14).stroke(p.border, lineWidth: 1))
        .padding(.horizontal, 16)
        .contentShape(Rectangle())
        .onTapGesture { open = true }
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(.isButton)
        .accessibilityLabel("Your stats, see all")
        .accessibilityIdentifier("homeStats")
        .sheet(isPresented: $open) { StatsSheet() }
        .task(id: "\(model.list.kind)-\(model.list.version)") {
            guard let src = model.repo as? InsightsSource else { return }
            data = try? await src.stats(kind: model.list.kind)
        }
    }
}
