import Testing
@testable import MomentumApp

@Test func demoHabitsHaveUniqueTitles() {
    let titles = Habit.demo.map(\.title)
    #expect(Set(titles).count == titles.count)
}
