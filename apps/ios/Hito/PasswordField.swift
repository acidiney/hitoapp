import SwiftUI

struct PasswordField: View {
    @Binding var password: String
    var newPassword = false
    var hint: String?
    var focus: FocusState<AccountField?>.Binding
    let submit: () -> Void
    @State private var shown = false

    var body: some View {
        FieldBox(label: "Password", focused: focus.wrappedValue == .password, hint: hint) {
            Group {
                if shown {
                    TextField("", text: $password)
                } else {
                    SecureField("", text: $password)
                }
            }
            .textContentType(newPassword ? .newPassword : .password)
            .textInputAutocapitalization(.never)
            .autocorrectionDisabled()
            .focused(focus, equals: .password)
            .submitLabel(.go)
            .onSubmit(submit)
            .frame(minHeight: 44)
            Button {
                shown.toggle()
            } label: {
                Image(systemName: shown ? "eye.slash" : "eye")
                    .frame(width: 44, height: 44)
                    .foregroundStyle(shown ? Theme.text : Theme.quiet)
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Show password")
            .accessibilityValue(shown ? "On" : "Off")
        }
    }
}

enum AccountField: Hashable {
    case username
    case password
}

struct UsernameField: View {
    @Binding var username: String
    var focus: FocusState<AccountField?>.Binding

    var body: some View {
        FieldBox(label: "Username", focused: focus.wrappedValue == .username) {
            TextField("", text: $username)
                .textContentType(.username)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .focused(focus, equals: .username)
                .submitLabel(.next)
                .onSubmit { focus.wrappedValue = .password }
                .frame(minHeight: 44)
        }
    }
}
