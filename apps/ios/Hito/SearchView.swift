import HitoKit
import SwiftUI

struct SearchView: View {
    @Environment(AppModel.self) private var model
    @State private var text = ""
    @State private var tracks = Feed<Track>()
    @FocusState private var focused: Bool

    var query: String { text.trimmingCharacters(in: .whitespacesAndNewlines) }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 10) {
                Image(systemName: "magnifyingglass").font(.system(size: 16, weight: .semibold)).foregroundStyle(Theme.quiet)
                TextField("", text: $text, prompt: Text("Search your library").foregroundStyle(Theme.quiet))
                    .accessibilityLabel("Search")
                    .focused($focused)
                    .submitLabel(.search)
                    .autocorrectionDisabled()
                    .textInputAutocapitalization(.never)
                if !text.isEmpty {
                    Button { text = "" } label: {
                        Image(systemName: "xmark").font(.system(size: 14, weight: .semibold))
                            .frame(width: 44, height: 44)
                    }
                    .buttonStyle(.plain)
                    .foregroundStyle(Theme.soft)
                    .accessibilityLabel("Clear search")
                }
            }
            .padding(.leading, 16)
            .frame(minHeight: 48)
            .background(Capsule().fill(Theme.field))
            .overlay(Capsule().strokeBorder(focused ? Theme.focus : Theme.line, lineWidth: focused ? 2 : 1))
            .padding(.horizontal, 16)
            .padding(.top, 12)
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 12) {
                    if query.isEmpty {
                        Text("Find tracks in your library by title, artist or album.")
                            .font(Theme.small)
                            .foregroundStyle(Theme.soft)
                    } else if tracks.isEmpty {
                        VStack(alignment: .leading, spacing: 6) {
                            Text("Nothing in your library matches “\(query)”").font(Theme.bodyStrong)
                            Text("Check the spelling, or try fewer words.").font(Theme.small).foregroundStyle(Theme.soft)
                        }
                    } else {
                        Text("Tracks").font(Theme.section).accessibilityAddTraits(.isHeader)
                        TrackRows(feed: tracks, context: "Search", empty: "")
                    }
                }
                .padding(.horizontal, 16)
                .padding(.bottom, 16)
            }
            .scrollIndicators(.hidden)
        }
        .task(id: query) {
            guard !query.isEmpty, let api = model.api else { return }
            try? await Task.sleep(for: .milliseconds(250))
            guard !Task.isCancelled else { return }
            let library = Library(api: api)
            let search = query
            tracks.reset { try await library.tracks(offset: $0, search: search) }
            await tracks.more(model)
        }
        .onAppear { focused = true }
    }
}
