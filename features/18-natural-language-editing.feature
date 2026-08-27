@F18 @edit
Feature: Apply scoped natural-language edits

  Scenario: Change color and timing in the edit fixture
    Given examples/fixtures/edit-debug/fixture.json is unchanged
    When the user asks "Make the ring gold and hold it two seconds longer"
    Then scene.color becomes "#FFD166"
    And scene.hold_seconds becomes 3.0
    And label, radius, and enter_seconds do not change
    And only invalidated output is rerendered

  Scenario: Edit one semantic object
    Given the project metadata declares semantic ID pulse.label
    When the user asks "Rename pulse.label to Matrix step"
    Then only scene.label becomes "Matrix step"
    And an undo checkpoint records the prior value

  Scenario: Apply a global theme edit
    Given all scenes consume theme tokens
    When the user selects the high-contrast theme
    Then every tokenized scene inherits the new palette
    And Codex reports any hard-coded color exceptions it finds
