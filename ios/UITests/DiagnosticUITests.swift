import XCTest

/// End to end on the Simulator, against scripts/e2e/mock_bluelink.py on
/// 127.0.0.1 (the app bundles the mock's constants for this build).
/// Run by scripts/ios-sim-test.sh. Account values match the mock's.
final class DiagnosticUITests: XCTestCase {
    private let app = XCUIApplication()

    override func setUpWithError() throws {
        continueAfterFailure = false
        app.launch()
    }

    func testCheckConnectionThenSendTestDestination() throws {
        let line = app.staticTexts["statusLine"]
        XCTAssertTrue(line.waitForExistence(timeout: 20), "the Diagnostic screen did not appear")
        XCTAssertTrue(line.label.contains("auth -"), "fresh line: \(line.label)")

        type("e2e@example.test", into: app.textFields["emailField"])
        type("e2e-Password-1", into: app.secureTextFields["passwordField"])
        type("1234", into: app.secureTextFields["pinField"])
        // Only shown with the software keyboard (not when a hardware one is attached).
        let done = app.buttons["keyboardDone"]
        if done.waitForExistence(timeout: 2) { done.tap() }
        tap(app.buttons["saveButton"])
        XCTAssertTrue(app.staticTexts["Saved."].waitForExistence(timeout: 5), "Keychain save failed")

        tap(app.buttons["checkButton"])
        wait(for: line, toContain: "auth ok · veh ok · soc 80 · send -")

        tap(app.buttons["sendButton"])
        wait(for: line, toContain: "auth ok · veh ok · soc 80 · send ok")
        XCTAssertTrue(app.staticTexts["All ok."].exists)
    }

    private func type(_ text: String, into field: XCUIElement) {
        XCTAssertTrue(field.waitForExistence(timeout: 5), "missing field \(field)")
        tap(field)
        field.typeText(text)
    }

    /// Scrolls the list until the element can be tapped.
    private func tap(_ element: XCUIElement) {
        XCTAssertTrue(element.waitForExistence(timeout: 5), "missing \(element)")
        var swipes = 0
        while !element.isHittable && swipes < 5 {
            app.swipeUp()
            swipes += 1
        }
        element.tap()
    }

    private func wait(for element: XCUIElement, toContain text: String) {
        let found = NSPredicate(format: "label CONTAINS %@", text)
        let expectation = XCTNSPredicateExpectation(predicate: found, object: element)
        let result = XCTWaiter().wait(for: [expectation], timeout: 60)
        XCTAssertEqual(result, .completed, "status line is \(element.label), wanted \(text)")
    }
}
