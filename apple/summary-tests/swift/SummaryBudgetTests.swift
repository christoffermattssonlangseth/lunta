// Test driver for SummaryThrottle and SummaryBudget in apple/Shared/SummaryWriter.swift (run by summary-budget.test.js).
import Foundation

var failures = 0
func check<T: Equatable>(_ got: T, _ want: T, _ what: String) {
    if got == want { print("ok   \(what)") } else { print("FAIL \(what): got \(got), want \(want)"); failures += 1 }
}

@main
struct Main {
    static func main() {
        // a summary written token by token: 60 partial texts in 3 s reach the page at most 4 times a second
        var t = SummaryThrottle(perSecond: SummaryBudget.updatesPerSecond)
        let t0 = Date(timeIntervalSince1970: 1000)
        check(t.allows(partial: false, now: t0), true, "loading always goes through")
        var drawn = 0
        for i in 0..<60 where t.allows(partial: true, now: t0.addingTimeInterval(0.05 * Double(i + 1))) { drawn += 1 }
        check(drawn <= 13 && drawn >= 11, true, "60 partial texts in 3 s: \(drawn) drawn (about 4 a second)")
        check(t.allows(partial: false, now: t0.addingTimeInterval(3.01)), true, "the finished summary always goes through")
        check(t.allows(partial: true, now: t0.addingTimeInterval(3.05)), false, "a partial right after it waits")
        check(t.allows(partial: true, now: t0.addingTimeInterval(3.30)), true, "a quarter of a second later it goes")

        // written without being asked: not in Low Power Mode, not when the device is hot
        check(SummaryBudget.allowsAutomatic(lowPower: false, thermal: .nominal), true, "nominal: yes")
        check(SummaryBudget.allowsAutomatic(lowPower: false, thermal: .fair), true, "fair: yes")
        check(SummaryBudget.allowsAutomatic(lowPower: true, thermal: .nominal), false, "Low Power Mode: no")
        check(SummaryBudget.allowsAutomatic(lowPower: false, thermal: .serious), false, "serious: no")
        check(SummaryBudget.allowsAutomatic(lowPower: false, thermal: .critical), false, "critical: no")
        check(SummaryBudget.stillBeforeStart >= 1000, true, "the page is still a second or more before the model starts")

        if failures == 0 { print("all passed") } else { print("\(failures) failed"); exit(1) }
    }
}
