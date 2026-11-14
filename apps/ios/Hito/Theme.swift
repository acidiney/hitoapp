import SwiftUI

enum Theme {
    static let ground = Color(red: 0x11 / 255, green: 0x11 / 255, blue: 0x10 / 255)
    static let field = Color(red: 0x1b / 255, green: 0x1b / 255, blue: 0x19 / 255)
    static let line = Color(red: 0x3a / 255, green: 0x3a / 255, blue: 0x36 / 255)
    static let text = Color(red: 0xf2 / 255, green: 0xf1 / 255, blue: 0xed / 255)
    static let soft = Color(red: 0xcf / 255, green: 0xce / 255, blue: 0xc8 / 255)
    static let quiet = Color(red: 0xa3 / 255, green: 0xa2 / 255, blue: 0x9c / 255)
    static let accent = Color(red: 0x6f / 255, green: 0x86 / 255, blue: 0xff / 255)
    static let focus = Color(red: 0x8f / 255, green: 0xa2 / 255, blue: 0xff / 255)
    static let onAccent = Color(red: 0x0c / 255, green: 0x0c / 255, blue: 0x0b / 255)
    static let alert = Color(red: 0xff / 255, green: 0x9f / 255, blue: 0x8a / 255)
    static let tile = Color(red: 0x2a / 255, green: 0x2a / 255, blue: 0x28 / 255)
    static let coverMark = Color(red: 0x8f / 255, green: 0x8e / 255, blue: 0x88 / 255)
    static let warning = Color(red: 0xe5 / 255, green: 0xb5 / 255, blue: 0x67 / 255)
    static let raised = Color(red: 0x26 / 255, green: 0x26 / 255, blue: 0x24 / 255)
    static let menu = Color(red: 0x2e / 255, green: 0x2e / 255, blue: 0x2b / 255)
    static let bar = Color(red: 0x0c / 255, green: 0x0c / 255, blue: 0x0b / 255)
    static let barLine = Color(red: 0x2a / 255, green: 0x2a / 255, blue: 0x27 / 255)
    static let stage = Color(red: 0x16 / 255, green: 0x16 / 255, blue: 0x15 / 255)
    static let selected = Color(red: 0x1c / 255, green: 0x21 / 255, blue: 0x40 / 255)
    static let danger = Color(red: 0xf6 / 255, green: 0xb8 / 255, blue: 0xa8 / 255)
    static let liked = Color(red: 0xf0 / 255, green: 0xb4 / 255, blue: 0x9a / 255)
    static let rest = Color(red: 0x4a / 255, green: 0x4a / 255, blue: 0x46 / 255)
    static let missing = Color(red: 0x8a / 255, green: 0x89 / 255, blue: 0x84 / 255)
    static let avatar = Color(red: 0x1f / 255, green: 0x4a / 255, blue: 0x43 / 255)

    static let body = Font.custom("Figtree-Regular", size: 16, relativeTo: .body)
    static let bodyStrong = Font.custom("Figtree-SemiBold", size: 16, relativeTo: .body)
    static let bodyBold = Font.custom("Figtree-Bold", size: 16, relativeTo: .body)
    static let bodyMedium = Font.custom("Figtree-Medium", size: 16, relativeTo: .body)
    static let small = Font.custom("Figtree-Regular", size: 14, relativeTo: .subheadline)
    static let smallMedium = Font.custom("Figtree-Medium", size: 14, relativeTo: .subheadline)
    static let smallStrong = Font.custom("Figtree-SemiBold", size: 14, relativeTo: .subheadline)
    static let title = Font.custom("Figtree-ExtraBold", size: 28, relativeTo: .largeTitle)
    static let pageTitle = Font.custom("Figtree-ExtraBold", size: 22, relativeTo: .title)
    static let section = Font.custom("Figtree-Bold", size: 20, relativeTo: .title2)
    static let playing = Font.custom("Figtree-Bold", size: 24, relativeTo: .title)
    static let label = Font.custom("Figtree-SemiBold", size: 12, relativeTo: .caption)
    static let caption = Font.custom("Figtree-Regular", size: 12, relativeTo: .caption)
    static let tab = Font.custom("Figtree-Medium", size: 11, relativeTo: .caption2)
    static let tabCurrent = Font.custom("Figtree-Bold", size: 11, relativeTo: .caption2)
    static let heading = Font.custom("Figtree-Bold", size: 18, relativeTo: .title3)
    static let result = Font.custom("Figtree-Bold", size: 20, relativeTo: .title2)
    static let lyric = Font.custom("Figtree-Bold", size: 22, relativeTo: .title2)
    static let lyricCurrent = Font.custom("Figtree-Bold", size: 32, relativeTo: .largeTitle)
}

struct Screen<Content: View>: View {
    @ViewBuilder let content: Content

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 40) {
                Logo()
                VStack(alignment: .leading, spacing: 18) { content }
            }
            .padding(.horizontal, 16)
            .padding(.top, 12)
            .padding(.bottom, 28)
            .frame(maxWidth: 440, alignment: .leading)
            .frame(maxWidth: .infinity)
        }
        .background(Theme.ground.ignoresSafeArea())
        .foregroundStyle(Theme.text)
    }
}

struct Logo: View {
    var body: some View {
        Image("Logo")
            .resizable()
            .scaledToFit()
            .frame(width: 87, height: 19)
            .frame(height: 44)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Hito")
    }
}

struct Heading: View {
    let title: String
    var address: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title)
                .font(Theme.title)
                .accessibilityAddTraits(.isHeader)
            if let address { AddressLine(text: address) }
        }
    }
}

struct AddressLine: View {
    let text: String

    var body: some View {
        HStack(spacing: 8) {
            Circle().fill(Theme.soft).frame(width: 9, height: 9).accessibilityHidden(true)
            Text(text).font(Theme.small).foregroundStyle(Theme.quiet)
        }
    }
}

struct FieldBox<Field: View>: View {
    let label: String
    var focused: Bool
    var hint: String?
    var hintID: String?
    @ViewBuilder let field: Field

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(label).font(Theme.smallMedium).foregroundStyle(Theme.soft)
            HStack(spacing: 0) { field }
                .padding(.leading, 14)
                .frame(minHeight: 48)
                .background(Theme.field, in: RoundedRectangle(cornerRadius: 10))
                .overlay(
                    RoundedRectangle(cornerRadius: 10)
                        .strokeBorder(focused ? Theme.focus : Theme.line, lineWidth: focused ? 2 : 1)
                )
            if let hint {
                Text(hint).font(Theme.small).foregroundStyle(Theme.quiet)
            }
        }
    }
}

struct PrimaryButton: View {
    let title: String
    var busyTitle: String?
    var busy = false
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 10) {
                if busy { ProgressView().tint(Theme.onAccent) }
                Text(busy ? (busyTitle ?? title) : title).font(Theme.bodyStrong)
            }
            .frame(maxWidth: .infinity, minHeight: 48)
            .background(Theme.accent, in: Capsule())
            .foregroundStyle(Theme.onAccent)
        }
        .buttonStyle(.plain)
    }
}

struct TextAction: View {
    let title: String
    var icon: String?
    var color = Theme.focus
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 4) {
                if let icon { Icon(icon) }
                Text(title)
            }
            .font(Theme.smallMedium)
            .frame(minHeight: 44)
            .foregroundStyle(color)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}

struct Alert: View {
    let lines: [String]

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            ForEach(lines, id: \.self) { Text($0) }
        }
        .foregroundStyle(Theme.alert)
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(.updatesFrequently)
    }
}

struct Icon: View {
    let name: String
    var size: CGFloat = 18

    init(_ name: String, size: CGFloat = 18) {
        self.name = name
        self.size = size
    }

    var body: some View {
        Image("Icon-\(name)")
            .resizable()
            .renderingMode(.template)
            .frame(width: size, height: size)
            .accessibilityHidden(true)
    }
}

struct IconButton: View {
    let icon: String
    let label: String
    var size: CGFloat = 44
    var glyph: CGFloat = 18
    var color = Theme.text
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Icon(icon, size: glyph)
                .foregroundStyle(color)
                .frame(width: size, height: size)
                .contentShape(Circle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(label)
    }
}
