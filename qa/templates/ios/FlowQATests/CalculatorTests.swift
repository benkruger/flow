import XCTest

final class CalculatorTests: XCTestCase {
    func testAdd() {
        let calc = Calculator()
        XCTAssertEqual(calc.add(2, 3), 5)
    }

    func testAddNegative() {
        let calc = Calculator()
        XCTAssertEqual(calc.add(2, -3), -1)
    }
}
