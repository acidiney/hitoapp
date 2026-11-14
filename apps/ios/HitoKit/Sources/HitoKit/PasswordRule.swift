public enum PasswordRule {
    public static let minimum = 12
    public static let maximum = 1024

    public static func isLongEnough(_ password: String) -> Bool {
        (minimum...maximum).contains(password.utf8.count)
    }

    public static func progress(_ password: String) -> String {
        let count = password.utf8.count
        if count == 0 { return "At least \(minimum) characters. A long phrase is easier to remember." }
        return count >= minimum ? "Long enough" : "\(count) of \(minimum) characters"
    }
}
