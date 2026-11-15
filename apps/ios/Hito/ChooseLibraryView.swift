import SwiftUI

struct ChooseLibraryView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        Screen {
            VStack(alignment: .leading, spacing: 6) {
                Heading(title: "Where is your music?")
                Text("Choose where Hito keeps its library.").foregroundStyle(Theme.soft)
            }
            VStack(spacing: 12) {
                Choice(
                    title: "On this phone",
                    detail: "A private library here, for your own folders. Other devices can't reach it.",
                    icon: "phone",
                    emphasized: true
                ) { model.chooseThisPhone() }
                Choice(
                    title: "On a Hito server",
                    detail: "A server you or someone else runs, at home or over a VPN.",
                    icon: "server",
                    emphasized: false
                ) { model.chooseServer() }
            }
            .accessibilityElement(children: .contain)
            .accessibilityLabel("Where Hito keeps its library")
            Text("You can change this later in Settings.")
                .font(Theme.small)
                .foregroundStyle(Theme.quiet)
        }
    }
}

private struct Choice: View {
    let title: String
    let detail: String
    let icon: String
    let emphasized: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 16) {
                Icon(icon, size: 22)
                    .foregroundStyle(Theme.soft)
                    .frame(width: 48, height: 48)
                    .background(Color(red: 0x2a / 255, green: 0x2a / 255, blue: 0x28 / 255), in: RoundedRectangle(cornerRadius: 10))
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 2) {
                    Text(title).font(Theme.bodyStrong).foregroundStyle(Theme.text)
                    Text(detail).font(Theme.small).foregroundStyle(Theme.quiet)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                Icon("chevron-right")
                    .foregroundStyle(Theme.quiet)
                    .accessibilityHidden(true)
            }
            .padding(16)
            .frame(minHeight: 88)
            .background(emphasized ? Theme.field : Color.clear, in: RoundedRectangle(cornerRadius: 16))
            .overlay(
                RoundedRectangle(cornerRadius: 16)
                    .strokeBorder(emphasized ? Theme.focus : Theme.line, lineWidth: emphasized ? 2 : 1)
            )
            .contentShape(RoundedRectangle(cornerRadius: 16))
        }
        .buttonStyle(.plain)
    }
}

struct ThisPhoneView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        Screen {
            TextAction(title: "Back", icon: "chevron-left", color: Theme.soft) { model.backToChoice() }
            Heading(title: "Hito can't keep a library on this phone yet")
            Text("For now, Hito on a phone plays from a Hito server.")
                .foregroundStyle(Theme.soft)
            PrimaryButton(title: "Connect to a server instead") { model.chooseServer() }
        }
    }
}
