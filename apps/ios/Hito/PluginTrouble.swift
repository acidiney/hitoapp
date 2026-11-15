import HitoKit

enum PluginTrouble {
    static func message(_ error: Error, otherwise: String) -> String {
        if case HitoAPI.Problem.credentialsRequired = error {
            return HitoAPI.Problem.credentialsMessage + " Open Plugins to sign in again."
        }
        return otherwise
    }
}
