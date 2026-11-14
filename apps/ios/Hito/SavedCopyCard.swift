import HitoKit
import SwiftUI

struct SavedCopyCard: View {
    @Environment(AppModel.self) private var model
    let track: Track
    var fill: Color = Theme.field
    @State private var copy: SavedCopy?
    @State private var copyBusy = false
    @State private var copyProblem: String?
    @State private var copyRequest = UUID()

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 12) {
                if copy?.state == .ready {
                    Circle().fill(Theme.text).frame(width: 10, height: 10)
                } else {
                    Circle().strokeBorder(copy?.state == .failed ? Theme.warning : Theme.soft, style: StrokeStyle(lineWidth: 1.5, dash: copy?.transferring == true ? [2, 2] : [])).frame(width: 10, height: 10)
                }
                VStack(alignment: .leading, spacing: 0) {
                    Text("Saved copy").font(Theme.bodyBold)
                    Text(copy?.summary ?? "Save a copy on your server to listen when the music service can't play it.")
                        .font(Theme.small)
                        .foregroundStyle(copy?.state == .failed ? Theme.warning : Theme.soft)
                        .fixedSize(horizontal: false, vertical: true)
                }
                Spacer(minLength: 0)
                Button { Task { await (copy == nil ? saveCopy() : removeCopy()) } } label: {
                    Text(copy == nil ? "Save" : copy?.transferring == true ? "Cancel" : "Remove")
                        .font(Theme.smallMedium)
                        .foregroundStyle(copy == nil ? Theme.stage : Theme.text)
                        .padding(.horizontal, 14)
                        .frame(minHeight: 44)
                        .background(Capsule().fill(copy == nil ? Theme.text : .clear))
                        .overlay(Capsule().strokeBorder(copy == nil ? .clear : Theme.text.opacity(0.4)))
                        .opacity(copyBusy ? 0.5 : 1)
                }
                .buttonStyle(.plain)
                .disabled(copyBusy)
                .accessibilityLabel(copy == nil ? "Save a copy on your server" : copy?.transferring == true ? "Cancel saving the copy" : "Remove the saved copy")
            }
            if let copy, copy.transferring {
                if let progress = copy.progress {
                    HStack(spacing: 10) {
                        GeometryReader { box in
                            ZStack(alignment: .leading) {
                                Capsule().fill(Theme.text.opacity(0.2))
                                Capsule().fill(Theme.text).frame(width: box.size.width * min(max(progress, 0), 1))
                            }
                        }
                        .frame(height: 6)
                        Text("\(Int((progress * 100).rounded()))%").font(Theme.caption).monospacedDigit()
                    }
                    .accessibilityElement(children: .ignore)
                    .accessibilityLabel("Saving the copy")
                    .accessibilityValue("\(Int((progress * 100).rounded())) percent")
                } else {
                    ProgressView().progressViewStyle(.linear).tint(Theme.text)
                }
                Text("Plays from the copy next time.").font(Theme.small).foregroundStyle(Theme.soft)
            }
            if let copyProblem {
                Text(copyProblem).font(Theme.small).foregroundStyle(Theme.warning)
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 12)
        .background(RoundedRectangle(cornerRadius: 16).fill(fill))
        .task(id: track.id) { await followCopy() }
    }

    private func followCopy() async {
        guard let api = model.api else { return }
        let copies = SavedCopies(api: api)
        while !Task.isCancelled {
            copy = try? await copies.status(track.id)
            guard copy?.transferring == true else { return }
            try? await Task.sleep(for: .seconds(2))
        }
    }

    private func saveCopy() async {
        guard let api = model.api else { return }
        copyBusy = true
        copyProblem = nil
        defer { copyBusy = false }
        do {
            _ = try await SavedCopies(api: api).save(track.id, request: copyRequest)
            copyRequest = UUID()
            await followCopy()
        } catch let refused as SavedCopies.Refused {
            copyRequest = UUID()
            copyProblem = refused.reason
        } catch {
            copyProblem = "The copy didn't start. Try again."
            model.handle(error)
        }
    }

    private func removeCopy() async {
        guard let api = model.api, let intent = copy?.intentId else { return }
        copyBusy = true
        copyProblem = nil
        defer { copyBusy = false }
        do {
            try await SavedCopies(api: api).remove(intent)
            copy = nil
        } catch {
            copyProblem = "The copy is still there. Try again."
            model.handle(error)
        }
    }
}
