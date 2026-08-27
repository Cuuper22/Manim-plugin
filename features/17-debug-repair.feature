@F17 @debug
Feature: Diagnose failures and repair only the responsible source

  Scenario: Diagnose the missing-asset fixture
    Given examples/fixtures/edit-debug/broken_scene.py
    When MissingAssetScene is rendered
    Then the diagnostic identifies missing-badge.svg as absent
    And it scopes the failure to MissingAssetScene.construct

  Scenario: Apply the expected surgical repair
    Given the missing-asset fixture has failed
    When automatic repair is authorized
    Then the repaired scene matches expected/repaired_scene.py behavior
    And healthy fixture files remain unchanged
    And a preview of MissingAssetScene completes

  Scenario: Repair a visual transform mismatch
    Given source and target formulas have incompatible automatic token matching
    When frame inspection detects the malformed transform
    Then an explicit token map or non-morphing transition is selected
    And only that transition is rerendered
