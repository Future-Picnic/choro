import Foundation

public struct Habit: Identifiable, Equatable, Sendable {
    public let id: UUID
    public var title: String
    public var symbol: String
    public var streak: Int
    public var completedToday: Bool

    public init(id: UUID = UUID(), title: String, symbol: String, streak: Int, completedToday: Bool) {
        self.id = id
        self.title = title
        self.symbol = symbol
        self.streak = streak
        self.completedToday = completedToday
    }
}

public extension Habit {
    static let demo = [
        Habit(title: "Morning walk", symbol: "figure.walk", streak: 12, completedToday: true),
        Habit(title: "Read for 20 minutes", symbol: "book.closed", streak: 6, completedToday: false),
        Habit(title: "Plan tomorrow", symbol: "checklist", streak: 3, completedToday: false)
    ]
}
