public enum PasswordRule {
    public static let minimum = 12
    public static let maximum = 1024

    public static func isLongEnough(_ password: String) -> Bool {
        problem(password) == nil
    }

    public static func problem(_ password: String) -> String? {
        let count = password.utf8.count
        if count > maximum { return "Too long. Use at most \(maximum) characters." }
        if count < minimum { return "Use a password of at least \(minimum) characters." }
        return nil
    }

    public static func progress(_ password: String) -> String {
        let count = password.utf8.count
        if count == 0 { return "At least \(minimum) characters. A long phrase is easier to remember." }
        if count > maximum { return "Too long. Use at most \(maximum) characters." }
        return count >= minimum ? "Long enough" : "\(count) of \(minimum) characters"
    }
}
