import HitoKit
import SwiftUI

struct ArtistSheet: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let recording: String
    let credit: String?
    let chosen: () -> Void
    @State private var providers: [MetadataProvider]?
    @State private var provider: MetadataProvider?
    @State private var session: ArtistReview.Session?
    @State private var query = ""
    @State private var candidates: [ArtistCandidate]?
    @State private var picked: ArtistCandidate?
    @State private var profile: ArtistProfile?
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
        .task {
            if query.isEmpty { query = credit ?? "" }
            await load()
        }
    }

    private var sheet: some View {
        VStack(alignment: .leading, spacing: 16) {
            Capsule().fill(Theme.line).frame(width: 40, height: 4).frame(maxWidth: .infinity)
            VStack(alignment: .leading, spacing: 4) {
                Text("Choose the artist").font(Theme.title).accessibilityAddTraits(.isHeader)
                Text("Hito uses the artist to find this album's editions.").font(Theme.small).foregroundStyle(Theme.quiet)
            }
            if let profile, let picked {
                review(profile, of: picked)
            } else {
                search
            }
            if let problem {
                Text(problem).font(Theme.small).foregroundStyle(Color(hex: 0xf6d3ca))
                    .padding(12)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(RoundedRectangle(cornerRadius: 10).fill(Color(hex: 0x2a1b17)))
            }
            Button { profile == nil ? dismiss() : back() } label: {
                Text(profile == nil ? "Cancel" : "Back to results")
                    .font(Theme.bodyMedium)
                    .foregroundStyle(Theme.text)
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

    @ViewBuilder private var search: some View {
        if let providers {
            if providers.isEmpty {
                Text("No metadata provider is on for your account, so there's no artist to choose from.")
                    .font(Theme.small)
                    .foregroundStyle(Theme.soft)
            } else {
                if providers.count > 1 {
                    Menu {
                        ForEach(providers) { option in
                            Button(option.name) { Task { await use(option) } }
                        }
                    } label: {
                        HStack(spacing: 8) {
                            Text("Provider").foregroundStyle(Theme.quiet)
                            Text(provider?.name ?? "Choose").font(Theme.bodyMedium)
                            Icon("chevron-down", size: 14)
                        }
                        .font(Theme.small)
                        .foregroundStyle(Theme.text)
                        .padding(.horizontal, 16)
                        .frame(minHeight: 44)
                        .overlay(Capsule().strokeBorder(Theme.line))
                    }
                    .disabled(working)
                }
                HStack(spacing: 12) {
                    Icon("search", size: 20).foregroundStyle(Theme.quiet)
                    TextField("", text: $query, prompt: Text("Artist name").foregroundStyle(Theme.quiet))
                        .autocorrectionDisabled()
                        .submitLabel(.search)
                        .onSubmit { Task { await find() } }
                }
                .padding(.horizontal, 16)
                .frame(minHeight: 52)
                .background(RoundedRectangle(cornerRadius: 12).fill(Theme.ground))
                .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Theme.line))
                PrimaryButton(title: "Search artists", busyTitle: "Searching artists…", busy: working && candidates == nil) { Task { await find() } }
                    .disabled(working || session == nil || query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                if let candidates {
                    if candidates.isEmpty {
                        Text("No artists found. Try another name.").font(Theme.small).foregroundStyle(Theme.soft)
                    } else {
                        ScrollView {
                            VStack(spacing: 0) {
                                ForEach(candidates) { candidate in
                                    Button { Task { await open(candidate) } } label: {
                                        HStack(spacing: 12) {
                                            VStack(alignment: .leading, spacing: 2) {
                                                Text(candidate.name).font(Theme.bodyMedium)
                                                if let detail = candidate.detail {
                                                    Text(detail).font(Theme.small).foregroundStyle(Theme.quiet).lineLimit(2)
                                                }
                                            }
                                            Spacer(minLength: 0)
                                            Icon("chevron-right", size: 16).foregroundStyle(Theme.quiet)
                                        }
                                        .frame(minHeight: 56)
                                        .contentShape(Rectangle())
                                    }
                                    .buttonStyle(.plain)
                                    .disabled(working)
                                }
                            }
                        }
                        .frame(maxHeight: 260)
                    }
                }
            }
        } else {
            ProgressView().tint(Theme.soft).frame(maxWidth: .infinity, minHeight: 60)
        }
    }

    private func review(_ profile: ArtistProfile, of candidate: ArtistCandidate) -> some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 14) {
                ZStack { Theme.tile; Icon("person", size: 24).foregroundStyle(Theme.quiet) }
                    .frame(width: 56, height: 56)
                    .clipShape(Circle())
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 2) {
                    Text(profile.name).font(Theme.bodyStrong)
                    Text([profile.detail, provider.map { "From \($0.name)" }].compactMap { $0 }.joined(separator: " · "))
                        .font(Theme.small)
                        .foregroundStyle(Theme.quiet)
                }
            }
            if let credit {
                Text("Your files credit \(credit).").font(Theme.small).foregroundStyle(Theme.soft)
            }
            PrimaryButton(title: "Use this artist", busyTitle: "Saving artist…", busy: working) { Task { await confirm(candidate) } }
                .disabled(working)
        }
    }

    private func load() async {
        guard let api = model.api else { return }
        do {
            let names = (try? await Plugins(api: api).load().connections).map { Dictionary($0.map { ($0.id, $0.name) }, uniquingKeysWith: { first, _ in first }) } ?? [:]
            let found = try await ArtistReview(api: api).providers(named: names)
            providers = found
            if let first = found.first { await use(first) }
        } catch {
            providers = []
            problem = "Metadata providers didn't load. Close this and try again."
            model.handle(error)
        }
    }

    private func use(_ chosen: MetadataProvider) async {
        guard let api = model.api else { return }
        provider = chosen
        session = nil
        candidates = nil
        problem = nil
        working = true
        defer { working = false }
        do {
            session = try await ArtistReview(api: api).start(recording: recording, provider: chosen)
        } catch let reason as ArtistReview.Problem {
            problem = reason.message
        } catch {
            problem = "\(chosen.name) can't be used for this album right now."
            model.handle(error)
        }
    }

    private func find() async {
        guard let api = model.api, let session else { return }
        problem = nil
        candidates = nil
        working = true
        defer { working = false }
        do {
            let (next, found) = try await ArtistReview(api: api).search(query, in: session)
            self.session = next
            candidates = found
        } catch let reason as ArtistReview.Problem {
            problem = reason.message
        } catch {
            problem = "The search didn't finish. Try again."
            model.handle(error)
        }
    }

    private func open(_ candidate: ArtistCandidate) async {
        guard let api = model.api, let session else { return }
        problem = nil
        working = true
        defer { working = false }
        do {
            let (next, shown) = try await ArtistReview(api: api).preview(candidate, in: session)
            self.session = next
            picked = candidate
            profile = shown
        } catch let reason as ArtistReview.Problem {
            problem = reason.message
        } catch {
            problem = "This artist didn't load. Try again."
            model.handle(error)
        }
    }

    private func confirm(_ candidate: ArtistCandidate) async {
        guard let api = model.api, let session else { return }
        problem = nil
        working = true
        defer { working = false }
        do {
            try await ArtistReview(api: api).confirm(candidate, in: session)
            chosen()
            dismiss()
        } catch let reason as ArtistReview.Problem {
            problem = reason.message
        } catch {
            problem = "The artist wasn't saved. Try again."
            model.handle(error)
        }
    }

    private func back() {
        profile = nil
        picked = nil
        problem = nil
    }
}
