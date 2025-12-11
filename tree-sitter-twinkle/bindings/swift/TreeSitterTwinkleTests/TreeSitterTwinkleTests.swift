import XCTest
import SwiftTreeSitter
import TreeSitterTwinkle

final class TreeSitterTwinkleTests: XCTestCase {
    func testCanLoadGrammar() throws {
        let parser = Parser()
        let language = Language(language: tree_sitter_twinkle())
        XCTAssertNoThrow(try parser.setLanguage(language),
                         "Error loading Twinkle grammar")
    }
}
