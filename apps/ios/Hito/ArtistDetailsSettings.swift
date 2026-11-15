import HitoKit
import SwiftUI

struct ArtistDetailsSettings: View {
    @Environment(AppModel.self) private var model
    @State private var saved: MetadataProviders.Preferences?
    @State private var draft: MetadataProviders.Preferences?
    @State private var connections: [PluginConnection] = []
    @State private var saving = false
    @State private var note: String?
    @State private var problem: String?
    @State private var removing: MetadataProviders.Provider?

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Artist details").font(Theme.section).accessibilityAddTraits(.isHeader)
            Text("File tags and your own edits are always kept.").font(Theme.small).foregroundStyle(Theme.quiet)
            if let draft {
                SettingRow(title: "Use metadata providers", detail: "Fills in artist profiles from the providers below, in this order.", divided: true) {
                    Switch(on: draft.enabled, label: "Use metadata providers") { on in self.draft?.enabled = on }
                }
                if draft.providers.isEmpty {
                    Text("No metadata providers selected.").font(Theme.small).foregroundStyle(Theme.soft).padding(.vertical, 8)
                }
                ForEach(Array(draft.providers.enumerated()), id: \.element) { index, provider in
                    HStack(spacing: 12) {
                        Text("\(index + 1)").font(Theme.smallStrong).foregroundStyle(Theme.quiet).frame(width: 22)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(provider.label(connections)).font(Theme.bodyMedium)
                            Text(provider.detail(connections)).font(Theme.small).foregroundStyle(Theme.quiet)
                        }
                        Spacer(minLength: 8)
                        Menu {
                            Button("Move earlier") { self.draft?.move(provider, by: -1) }.disabled(index == 0)
                            Button("Move later") { self.draft?.move(provider, by: 1) }.disabled(index == draft.providers.count - 1)
                            Button("Remove", role: .destructive) { removing = provider }
                        } label: {
                            Icon("more-vertical").foregroundStyle(Theme.quiet).frame(width: 44, height: 44).contentShape(Circle())
                        }
                        .accessibilityLabel("Options for \(provider.label(connections))")
                    }
                    .frame(minHeight: 56)
                    .opacity(draft.enabled ? 1 : 0.5)
                    .overlay(alignment: .bottom) { Theme.barLine.frame(height: 1) }
                }
                let available = draft.available(connections)
                if available.isEmpty {
                    Text("Every available metadata provider is in use.").font(Theme.small).foregroundStyle(Theme.soft).padding(.top, 4)
                } else if draft.canAdd {
                    Menu {
                        ForEach(available, id: \.self) { provider in
                            Button(provider.label(connections)) { self.draft?.providers.append(provider) }
                        }
                    } label: {
                        Text("Add a metadata provider").font(Theme.smallStrong).foregroundStyle(Theme.focus).frame(minHeight: 44)
                    }
                }
                if draft != saved {
                    PrimaryButton(title: "Save metadata preferences", busyTitle: "Saving metadata preferences…", busy: saving) { Task { await save(draft) } }
                        .disabled(saving || (draft.enabled && draft.providers.isEmpty))
                        .padding(.top, 6)
                }
                if let note { Text(note).font(Theme.small).foregroundStyle(Theme.soft) }
            } else if problem == nil {
                Text("Loading metadata preferences…").font(Theme.small).foregroundStyle(Theme.soft)
            }
            if let problem {
                Text(problem).font(Theme.small).foregroundStyle(Theme.alert)
                TextAction(title: "Reload metadata preferences") { Task { await load() } }
            }
        }
        .task { await load() }
        .confirmationDialog("Remove \(removing?.label(connections) ?? "provider")?", isPresented: Binding(get: { removing != nil }, set: { if !$0 { removing = nil } }), titleVisibility: .visible) {
            Button("Remove", role: .destructive) {
                if let removing { draft?.providers.removeAll { $0 == removing } }
                removing = nil
            }
        } message: {
            Text("It stops being asked for artist profiles once you save. You can add it again later.")
        }
    }

    private func load() async {
        guard let api = model.api else { return }
        do {
            async let preferences = MetadataProviders(api: api).load()
            async let plugins = Plugins(api: api).load()
            let (loaded, state) = try await (preferences, plugins)
            saved = loaded
            draft = loaded
            connections = state.connections
            problem = nil
        } catch {
            problem = "Metadata preferences didn't load."
        }
    }

    private func save(_ next: MetadataProviders.Preferences) async {
        guard let api = model.api else { return }
        saving = true
        defer { saving = false }
        do {
            let stored = try await MetadataProviders(api: api).save(next)
            saved = stored
            draft = stored
            note = "Metadata preferences saved."
            problem = nil
        } catch MetadataProviders.Problem.changed {
            note = nil
            problem = "These preferences changed somewhere else. Here are the latest."
            await load()
        } catch {
            note = nil
            problem = "Metadata preferences weren't saved. Try again."
        }
    }
}
