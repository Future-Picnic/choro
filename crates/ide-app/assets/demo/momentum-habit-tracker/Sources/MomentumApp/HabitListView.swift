import SwiftUI

public struct HabitListView: View {
    @State private var habits = Habit.demo

    public init() {}

    public var body: some View {
        NavigationStack {
            List($habits) { $habit in
                Button {
                    habit.completedToday.toggle()
                } label: {
                    HStack(spacing: 14) {
                        Image(systemName: habit.symbol)
                            .frame(width: 34, height: 34)
                            .background(.indigo.opacity(0.12), in: RoundedRectangle(cornerRadius: 10))
                        VStack(alignment: .leading) {
                            Text(habit.title).font(.headline)
                            Text("\(habit.streak) day rhythm").font(.caption).foregroundStyle(.secondary)
                        }
                        Spacer()
                        Image(systemName: habit.completedToday ? "checkmark.circle.fill" : "circle")
                            .foregroundStyle(habit.completedToday ? .indigo : .secondary)
                    }
                }
                .buttonStyle(.plain)
                .accessibilityValue(habit.completedToday ? "Completed today" : "Not completed today")
            }
            .navigationTitle("Today")
        }
    }
}

#Preview { HabitListView() }
