import HitoKit
import SwiftUI

struct ConnectSheet: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let offer: PluginOffer
    let connected: () -> Void
    @State private var token = ""
    @State private var preview: PluginPreview?
    @State private var working = false
    @State private var problem: String?

    var body: some View {
        VStack(spacing: 0) {
            Color.black.opacity(0.55)
                .onTapGesture { dismiss() }
                .accessibilityHidden(true)
            sheet
        }
        .ignoresSafeArea(edges: .top)
        .onChange(of: token) { preview = nil; problem = nil }
    }

    private var sheet: some View {
        VStack(alignment: .leading, spacing: 16) {
            Capsule().fill(Theme.line).frame(width: 40, height: 4).frame(maxWidth: .infinity)
            HStack(spacing: 12) {
                PluginTile(name: offer.name)
                Text("Connect \(offer.name)").font(Theme.pageTitle).lineLimit(2).accessibilityAddTraits(.isHeader)
            }
            VStack(alignment: .leading, spacing: 8) {
                Text("Access key").font(Theme.small).foregroundStyle(Theme.quiet)
                SecureField("", text: $token, prompt: Text("Only if \(offer.name) asks for one").foregroundStyle(Theme.quiet))
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .padding(.horizontal, 16)
                    .frame(minHeight: 52)
                    .background(RoundedRectangle(cornerRadius: 12).fill(Theme.ground))
                    .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Theme.line))
            }
            if let preview {
                VStack(alignment: .leading, spacing: 6) {
                    Text("Turns on: \(preview.turnsOn.isEmpty ? "nothing Hito uses yet" : preview.turnsOn.joined(separator: ", "))")
                        .font(Theme.bodyMedium)
                        .fixedSize(horizontal: false, vertical: true)
                    if preview.signsIn {
                        Text("It also signs in to your \(preview.name) account. Do that on the web after connecting.")
                            .font(Theme.small).foregroundStyle(Theme.soft)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                    Text("Hito starts using \(preview.name) for these as soon as you connect. You can disconnect it at any time.")
                        .font(Theme.small).foregroundStyle(Theme.soft)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .padding(14)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(RoundedRectangle(cornerRadius: 14).fill(Theme.raised))
            }
            if let problem {
                Text(problem).font(Theme.small).foregroundStyle(Color(hex: 0xf6d3ca))
                    .padding(12)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(RoundedRectangle(cornerRadius: 10).fill(Color(hex: 0x2a1b17)))
            }
            Button { Task { await (preview == nil ? check() : connect()) } } label: {
                Text(label)
                    .font(Theme.bodyStrong)
                    .foregroundStyle(Theme.onAccent)
                    .frame(maxWidth: .infinity, minHeight: 52)
                    .background(Capsule().fill(Theme.accent))
                    .opacity(working ? 0.6 : 1)
            }
            .buttonStyle(.plain)
            .disabled(working)
            Button { dismiss() } label: {
                Text("Cancel").font(Theme.bodyMedium).foregroundStyle(Theme.text)
                    .frame(maxWidth: .infinity, minHeight: 52)
                    .overlay(Capsule().strokeBorder(Theme.line))
            }
            .buttonStyle(.plain)
        }
        .padding(.horizontal, 16)
        .padding(.top, 8)
        .padding(.bottom, 12)
        .foregroundStyle(Theme.text)
        .background(UnevenRoundedRectangle(topLeadingRadius: 16, topTrailingRadius: 16).fill(Theme.field).ignoresSafeArea(edges: .bottom))
        .accessibilityAddTraits(.isModal)
    }

    private var label: String {
        if working { return preview == nil ? "Checking…" : "Connecting…" }
        return preview == nil ? "Check what it turns on" : "Connect"
    }

    private func check() async {
        guard let api = model.api else { return }
        working = true
        problem = nil
        defer { working = false }
        do {
            preview = try await Plugins(api: api).preview(offer, token: token)
        } catch {
            problem = explain(error)
        }
    }

    private func connect() async {
        guard let api = model.api, let preview else { return }
        working = true
        problem = nil
        defer { working = false }
        do {
            _ = try await Plugins(api: api).connect(offer, token: token, digest: preview.digest)
            connected()
            dismiss()
        } catch {
            self.preview = nil
            problem = explain(error)
        }
    }

    private func explain(_ error: Error) -> String {
        switch error as? HitoAPI.Problem {
        case .signedOut?, .locked?:
            model.handle(error)
            return "Sign in again to connect \(offer.name)."
        case .refused(401)?, .refused(403)?:
            return "\(offer.name) didn't accept that access key. Check it and try again."
        case .refused(409)?:
            return "\(offer.name) changed on the server. Close this and try again."
        case .refused(422)?:
            return "\(offer.name) isn't a plugin Hito can use. Nothing was connected."
        case .unexpected(502)?, .noAnswer?:
            return "\(offer.name) didn't answer. Check that its service is running, then try again."
        default:
            return "\(offer.name) isn't connected. Try again."
        }
    }
}
