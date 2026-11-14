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
        .scrollDismissesKeyboard(.interactively)
        .background(Theme.ground.ignoresSafeArea())
        .foregroundStyle(Theme.text)
    }
}

struct Logo: View {
    var body: some View {
        HStack(spacing: 6) {
            Circle().fill(Theme.text).frame(width: 10, height: 10)
            Circle().stroke(Theme.accent, lineWidth: 2).frame(width: 8, height: 8)
            ZStack {
                ForEach(0..<8) { index in
                    Circle().fill(Theme.quiet).frame(width: 2, height: 2)
                        .offset(y: -4)
                        .rotationEffect(.degrees(Double(index) * 45 + 22.5))
                }
            }
            .frame(width: 10, height: 10)
            Text("hito").font(.system(size: 19, weight: .bold)).padding(.leading, 4)
        }
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
                .font(.system(size: 28, weight: .heavy))
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
            Text(text).font(.subheadline).foregroundStyle(Theme.quiet)
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
            Text(label).font(.subheadline.weight(.medium)).foregroundStyle(Theme.soft)
            HStack(spacing: 0) { field }
                .padding(.leading, 14)
                .frame(minHeight: 48)
                .background(Theme.field, in: RoundedRectangle(cornerRadius: 10))
                .overlay(
                    RoundedRectangle(cornerRadius: 10)
                        .strokeBorder(focused ? Theme.focus : Theme.line, lineWidth: focused ? 2 : 1)
                )
            if let hint {
                Text(hint).font(.subheadline).foregroundStyle(Theme.quiet)
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
                Text(busy ? (busyTitle ?? title) : title).fontWeight(.semibold)
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
                if let icon { Image(systemName: icon).font(.subheadline.weight(.semibold)) }
                Text(title)
            }
            .font(.subheadline.weight(.medium))
            .frame(minHeight: 44)
            .foregroundStyle(color)
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
