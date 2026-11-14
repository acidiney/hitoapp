import CoreText
import SwiftUI

@main
struct HitoApp: App {
    @State private var model = AppModel()
    @Environment(\.scenePhase) private var phase

    init() {
        if let font = Bundle.main.url(forResource: "Figtree", withExtension: "woff2") {
            CTFontManagerRegisterFontsForURL(font as CFURL, .process, nil)
        }
    }

    var body: some Scene {
        WindowGroup {
            RootView()
                .environment(model)
                .preferredColorScheme(.dark)
                .font(Theme.body)
                .task { await model.open() }
                .onChange(of: phase) { _, phase in
                    if phase == .active { Task { await model.checkLock() } }
                }
        }
    }
}
